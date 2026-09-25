//! A one-character test world: what the Phase 4 spell context will be, built on
//! [`CharacterSpells`] and a small spell db whose records are copied from the exported
//! `data/spells/warrior.yaml` (build 1.60.1.70009) with the matching overrides. Shared by the
//! spell runtime, proc and registry tests.

use std::collections::VecDeque;

use crate::buff::{Buff, BuffApplication, BuffContext, ChargeUse};
use crate::character_spells::{AddedSpell, BuffSlot, CharacterSpells, SharedBuffs};
use crate::combat_roll::{IncludedOutcomes, PhysicalAttackResult};
use crate::cooldown::CooldownControl;
use crate::effect::EffectHost;
use crate::engine::{Engine, Event, EventKind};
use crate::ids::{BuffId, CharId, CooldownId, InstanceId, SpellId};
use crate::proc::{ProcHost, ProcSource};
use crate::raid::SharedBuffRegistry;
use crate::resource::ResourceType;
use crate::spell::dbc::AuraState;
use crate::spell::modifiers::SpellModifiers;
use crate::spell::overrides::{OverrideFile, Overrides};
use crate::spell::periodic::TickReport;
use crate::spell::record::{EquippedItems, SpellDb, SpellFile};
use crate::spell::{CastReport, Hand, Spell, SpellHost, SpellStatus};
use crate::stance::Stance;
use crate::stats::CharacterStats;
use crate::target::Target;

/// Records copied from the pruned export (descriptions and labels dropped).
pub(crate) const SPELLS_YAML: &str = r#"
build: 1.60.1.70009
class: WARRIOR
spells:
- id: 78
  name: Heroic Strike
  rank_text: Rank 1
  skill_line: 26
  class_mask: 1
  acquire_method: 2
  attributes: [327700, 134217728, 0, 1024, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  range_yd: 5.0
  power:
  - {type: RAGE, cost: 150}
  categories: {defense_type: MELEE}
  levels: {base: 1, spell: 1}
  class_options:
    set: 4
    mask: [64, 0, 0, 0]
  equipped_items: {class: 2, subclass_mask: 173555}
  effects:
  - index: 0
    effect: WEAPON_DAMAGE_NOSCHOOL
    base_points: 11.0
    bonus_coefficient: 1.0
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
- id: 284
  name: Heroic Strike
  rank_text: Rank 2
  skill_line: 26
  class_mask: 1
  supercedes: 78
  attributes: [327700, 134217728, 0, 1024, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  range_yd: 5.0
  power:
  - {type: RAGE, cost: 150}
  categories: {defense_type: MELEE}
  levels: {base: 8, spell: 8}
  class_options:
    set: 4
    mask: [64, 0, 0, 0]
  equipped_items: {class: 2, subclass_mask: 173555}
  effects:
  - index: 0
    effect: WEAPON_DAMAGE_NOSCHOOL
    base_points: 21.0
    bonus_coefficient: 1.0
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
- id: 23881
  name: Bloodthirst
  rank_text: Rank 1
  skill_line: 256
  class_mask: 1
  attributes: [327696, 134218240, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  duration_ms: 10000
  range_yd: 5.0
  power:
  - {type: RAGE, cost: 300}
  cooldown: {category_recovery_ms: 6000, start_recovery_ms: 1500}
  categories: {category: 971, start_recovery_category: 133, defense_type: MELEE}
  levels: {base: 40, spell: 40}
  class_options:
    set: 4
    mask: [33554432, 1024, 0, 0]
  effects:
  - index: 0
    effect: SCHOOL_DAMAGE
    base_points: 30.0
    bonus_coefficient: 1.0
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
  - index: 1
    effect: DUMMY
    base_points: 35.0
    bonus_coefficient: 1.0
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
- id: 20662
  name: Execute
  rank_text: Rank 5
  skill_line: 256
  class_mask: 1
  attributes: [327952, 134218240, 0, 197632, 512, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  range_yd: 5.0
  power:
  - {type: RAGE, cost: 150}
  cooldown: {start_recovery_ms: 1500}
  categories: {start_recovery_category: 133, defense_type: MELEE}
  shapeshift_mask: 327680
  levels: {base: 56, spell: 56}
  class_options:
    set: 4
    mask: [536870912, 0, 0, 0]
  equipped_items: {class: 2, subclass_mask: 173555}
  aura_restrictions: {target_aura_state: WOUNDED_20_PERCENT}
  effects:
  - index: 0
    effect: DUMMY
    base_points: 600.0
    chain_amplitude: 1.5
    bonus_coefficient: 1.0
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
  - index: 1
    effect: TRIGGER_SPELL
    trigger_spell: 26651
    implicit_target: [UNIT_CASTER, NONE]
- id: 26651
  name: Execute
  attributes: [384, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  duration_ms: 1000
  shapeshift_mask: 1
  class_options: {set: 4}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: DUMMY
    base_points: 1.0
    bonus_coefficient: 1.0
    implicit_target: [UNIT_CASTER, NONE]
- id: 11551
  name: Battle Shout
  rank_text: Rank 6
  skill_line: 256
  class_mask: 1
  attributes: [327696, 0, 0, 256, 0, 0, 0, 0, 4096, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  duration_ms: 180000
  power:
  - {type: RAGE, cost: 100}
  cooldown: {start_recovery_ms: 1500}
  categories: {start_recovery_category: 133, defense_type: MAGIC}
  levels: {base: 52, spell: 52, max: 61}
  class_options:
    set: 4
    mask: [65536, 0, 0, 0]
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: MOD_ATTACK_POWER
    base_points: 111.0
    real_points_per_level: 0.6
    radius_yd: [20.0, 0.0]
    implicit_target: [UNIT_CASTER_AREA_PARTY, NONE]
- id: 25289
  name: Battle Shout
  rank_text: Rank 7
  skill_line: 256
  class_mask: 1
  supercedes: 11551
  attributes: [327696, 0, 0, 256, 0, 0, 0, 0, 4096, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  duration_ms: 180000
  power:
  - {type: RAGE, cost: 100}
  cooldown: {start_recovery_ms: 1500}
  categories: {start_recovery_category: 133, defense_type: MAGIC}
  levels: {base: 60, spell: 60, max: 61}
  class_options:
    set: 4
    mask: [65536, 0, 0, 0]
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: MOD_ATTACK_POWER
    base_points: 139.0
    radius_yd: [20.0, 0.0]
    implicit_target: [UNIT_CASTER_AREA_PARTY, NONE]
- id: 2457
  name: Battle Stance
  skill_line: 26
  class_mask: 1
  acquire_method: 2
  attributes: [151322640, 2415919104, 257, 1048576, 0, 0, 0, 0, 0, 0, 0, 32768, 0, 0, 0, 8192, 0]
  school_mask: 1
  duration_ms: -1
  power:
  - {type: RAGE}
  cooldown: {category_recovery_ms: 1000}
  categories: {category: 47, defense_type: MELEE}
  levels: {base: 1, spell: 1}
  aura_options: {proc_chance: 101}
  class_options:
    set: 4
    mask: [8388608, 0, 0, 0]
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: MOD_SHAPESHIFT
    misc_value: [17, 0]
    implicit_target: [UNIT_CASTER, NONE]
- id: 2458
  name: Berserker Stance
  skill_line: 256
  class_mask: 1
  attributes: [151322640, 268435456, 257, 1048576, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  duration_ms: -1
  power:
  - {type: RAGE}
  cooldown: {category_recovery_ms: 1000}
  categories: {category: 47, defense_type: MELEE}
  levels: {base: 30, spell: 30}
  aura_options: {proc_chance: 101}
  class_options:
    set: 4
    mask: [8388608, 0, 0, 0]
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: MOD_SHAPESHIFT
    misc_value: [19, 0]
    implicit_target: [UNIT_CASTER, NONE]
- id: 21156
  name: Battle Stance Passive
  skill_line: 26
  class_mask: 1
  attributes: [327888, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  duration_ms: -1
  levels: {spell: 1}
  aura_options: {proc_chance: 101}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: MOD_THREAT
    base_points: -20.0
    misc_value: [127, 0]
    implicit_target: [UNIT_CASTER, NONE]
- id: 7381
  name: Berserker Stance Passive
  skill_line: 256
  class_mask: 1
  attributes: [327888, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  power:
  - {type: RAGE}
  levels: {base: 30, spell: 30}
  class_options:
    set: 4
    mask: [0, 2048, 0, 0]
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: MOD_CRIT_PCT
    base_points: 3.0
    implicit_target: [UNIT_CASTER, NONE]
  - index: 1
    effect: APPLY_AURA
    aura: MOD_DAMAGE_PERCENT_TAKEN
    base_points: 10.0
    misc_value: [127, 0]
    implicit_target: [UNIT_CASTER, NONE]
  - index: 2
    effect: APPLY_AURA
    aura: MOD_THREAT
    base_points: -20.0
    misc_value: [127, 0]
    implicit_target: [UNIT_CASTER, NONE]
  - index: 3
    effect: APPLY_AURA
    aura: MOD_ATTACK_POWER_PCT
    implicit_target: [UNIT_CASTER, NONE]
- id: 11585
  name: Overpower
  rank_text: Rank 4
  skill_line: 26
  class_mask: 1
  attributes: [2424848, 134218240, 0, 0, 512, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8388608, 8192, 0]
  school_mask: 1
  range_yd: 5.0
  power:
  - {type: RAGE, cost: 50}
  - {type: COMBO_POINTS, cost: 1}
  cooldown: {category_recovery_ms: 5000, start_recovery_ms: 1500}
  categories: {category: 65, start_recovery_category: 133, defense_type: MELEE}
  shapeshift_mask: 65536
  levels: {base: 60, spell: 60}
  class_options:
    set: 4
    mask: [4, 0, 0, 0]
  equipped_items: {class: 2, subclass_mask: 173555}
  effects:
  - index: 0
    effect: NORMALIZED_WEAPON_DMG
    base_points: 35.0
    bonus_coefficient: 1.0
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
- id: 25288
  name: Revenge
  rank_text: Rank 6
  skill_line: 257
  class_mask: 1
  attributes: [327696, 134218240, 0, 1024, 512, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  range_yd: 5.0
  power:
  - {type: RAGE, cost: 50}
  cooldown: {category_recovery_ms: 5000, start_recovery_ms: 1500}
  categories: {category: 65, start_recovery_category: 133, defense_type: MELEE}
  shapeshift_mask: 131072
  levels: {base: 60, spell: 60}
  class_options:
    set: 4
    mask: [1024, 0, 0, 0]
  equipped_items: {class: 2, subclass_mask: 173555}
  aura_restrictions: {caster_aura_state: DEFENSIVE}
  effects:
  - index: 0
    effect: SCHOOL_DAMAGE
    base_points: 153.0
    variance: 0.2
    bonus_coefficient: 1.0
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
- id: 2687
  name: Bloodrage
  skill_line: 257
  class_mask: 1
  attributes: [327696, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  power:
  - {type: HEALTH, cost_pct: 20.0}
  cooldown: {recovery_ms: 60000}
  levels: {base: 10, spell: 10}
  class_options:
    set: 4
    mask: [256, 0, 0, 0]
  effects:
  - index: 0
    effect: ENERGIZE
    base_points: 100.0
    misc_value: [1, 0]
    implicit_target: [UNIT_CASTER, NONE]
  - index: 1
    effect: TRIGGER_SPELL
    trigger_spell: 29131
    implicit_target: [UNIT_CASTER, NONE]
- id: 29131
  name: Bloodrage
  attributes: [327680, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  duration_ms: 10000
  power:
  - {type: HEALTH}
  levels: {base: 10, spell: 10}
  class_options:
    set: 4
    mask: [256, 0, 0, 0]
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: PERIODIC_ENERGIZE
    base_points: 10.0
    aura_period_ms: 1000
    misc_value: [1, 0]
    implicit_target: [UNIT_CASTER, NONE]
- id: 11574
  name: Rend
  rank_text: Rank 7
  skill_line: 26
  class_mask: 1
  attributes: [327696, 134218240, 0, 1024, 0, 0, 0, 0, 4608, 0, 0, 0, 0, 128, 0, 8192, 0]
  school_mask: 1
  duration_ms: 21000
  range_yd: 5.0
  power:
  - {type: RAGE, cost: 100}
  cooldown: {start_recovery_ms: 1500}
  categories: {start_recovery_category: 133, defense_type: MELEE, mechanic: BLEED}
  shapeshift_mask: 196608
  levels: {base: 60, spell: 60}
  class_options:
    set: 4
    mask: [32, 0, 0, 0]
  equipped_items: {class: 2, subclass_mask: 173555}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: PERIODIC_DAMAGE
    base_points: 21.0
    aura_period_ms: 3000
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
- id: 11597
  name: Sunder Armor
  rank_text: Rank 5
  skill_line: 257
  class_mask: 1
  attributes: [327696, 134218240, 0, 1024, 1048576, 0, 0, 0, 0, 0, 0, 0, 0, 128, 0, 8192, 0]
  school_mask: 1
  duration_ms: 30000
  range_yd: 5.0
  power:
  - {type: RAGE, cost: 150}
  cooldown: {start_recovery_ms: 1500}
  categories: {start_recovery_category: 133, defense_type: MELEE}
  levels: {base: 58, spell: 58}
  aura_options: {proc_chance: 101, max_stacks: 5}
  class_options:
    set: 4
    mask: [16384, 0, 0, 0]
  equipped_items: {class: 2, subclass_mask: 173555}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: MOD_RESISTANCE
    base_points: -450.0
    misc_value: [1, 0]
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
  - index: 1
    effect: THREAT
    base_points: 1013.0
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
- id: 1464
  name: Slam
  rank_text: Rank 2
  skill_line: 256
  class_mask: 1
  attributes: [327696, 134218240, 0, 1024, 0, 0, 33554432, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  cast_time_ms: 1500
  range_yd: 5.0
  power:
  - {type: RAGE, cost: 150}
  cooldown: {category_recovery_ms: 15000, start_recovery_ms: 1500}
  categories: {category: 2412, start_recovery_category: 133, defense_type: MELEE}
  levels: {base: 30, spell: 30}
  class_options:
    set: 4
    mask: [2097152, 0, 0, 0]
  equipped_items: {class: 2, subclass_mask: 173555}
  effects:
  - index: 0
    effect: WEAPON_DAMAGE_NOSCHOOL
    base_points: 32.0
    bonus_coefficient: 1.0
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
- id: 1310197
  name: Slam
  rank_text: Rank 2
  attributes: [327696, 134218240, 0, 1024, 0, 0, 33554432, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  cast_time_ms: 1500
  range_yd: 5.0
  power:
  - {type: RAGE, cost: 150}
  cooldown: {category_recovery_ms: 15000, start_recovery_ms: 1500}
  categories: {category: 2412, start_recovery_category: 133, defense_type: MELEE}
  levels: {base: 30, spell: 30}
  class_options:
    set: 4
    mask: [2097152, 0, 0, 0]
  equipped_items: {class: 2, subclass_mask: 173555}
  effects:
  - index: 0
    effect: WEAPON_DAMAGE_NOSCHOOL
    base_points: 32.0
    bonus_coefficient: 1.0
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
- id: 12296
  name: Anger Management
  skill_line: 26
  attributes: [262352, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  levels: {base: 1, spell: 1}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: MOD_POWER_REGEN
    base_points: 15.0
    misc_value: [1, 0]
    implicit_target: [UNIT_CASTER, NONE]
  - index: 1
    effect: APPLY_AURA
    aura: DUMMY
    base_points: 1.0
    implicit_target: [UNIT_CASTER, NONE]
  - index: 2
    effect: APPLY_AURA
    aura: DUMMY
    base_points: 3.0
    implicit_target: [UNIT_CASTER, NONE]
  - index: 3
    effect: APPLY_AURA
    aura: DUMMY
    base_points: 30.0
    implicit_target: [UNIT_CASTER, NONE]
- id: 12319
  name: Flurry
  skill_line: 256
  attributes: [262336, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  levels: {base: 1, spell: 1}
  aura_options: {proc_chance: 100, proc_type_mask: 87380}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: DUMMY
    base_points: 1.0
    implicit_target: [UNIT_CASTER, NONE]
- id: 12966
  name: Flurry
  skill_line: 256
  acquire_method: 3
  attributes: [262144, 0, 0, 0, 0, 0, 0, 0, 4096, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  duration_ms: 15000
  levels: {base: 1, spell: 1}
  aura_options: {proc_chance: 100, proc_charges: 3, proc_type_mask: 4}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: MOD_MELEE_HASTE_3
    base_points: 30.0
    implicit_target: [UNIT_CASTER, NONE]
- id: 12834
  name: Deep Wounds
  skill_line: 26
  attributes: [464, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  levels: {base: 1, spell: 1}
  aura_options: {proc_chance: 100, proc_type_mask: 69972}
  class_options: {set: 4}
  equipped_items: {class: 2, subclass_mask: 173555}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: PROC_TRIGGER_SPELL
    trigger_spell: 12162
    implicit_target: [UNIT_CASTER, NONE]
- id: 12162
  name: Deep Wounds
  attributes: [262544, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  range_yd: 50000.0
  categories: {mechanic: BLEED}
  levels: {base: 1, spell: 1}
  effects:
  - index: 0
    effect: DUMMY
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
- id: 412609
  name: Deep Wound
  attributes: [16, 0, 4, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 0, 8192, 0]
  school_mask: 1
  duration_ms: 12000
  range_yd: 50000.0
  categories: {mechanic: BLEED}
  levels: {base: 1, spell: 1}
  aura_options: {proc_chance: 101}
  class_options:
    set: 4
    mask: [0, 16, 0, 0]
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: PERIODIC_DUMMY
    base_points: 1.0
    aura_period_ms: 3000
    bonus_coefficient: 1.0
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
- id: 12322
  name: Unbridled Wrath
  skill_line: 256
  attributes: [464, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  levels: {base: 1, spell: 1}
  aura_options: {proc_chance: 60, proc_type_mask: 4}
  equipped_items: {class: 2, subclass_mask: 173555}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: PROC_TRIGGER_SPELL
    trigger_spell: 12964
    implicit_target: [UNIT_CASTER, NONE]
- id: 12964
  name: Unbridled Wrath
  skill_line: 256
  attributes: [262160, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  levels: {base: 1, spell: 1}
  effects:
  - index: 0
    effect: ENERGIZE
    base_points: 10.0
    misc_value: [1, 0]
    implicit_target: [UNIT_CASTER, NONE]
- id: 12292
  name: Sweeping Strikes
  skill_line: 26
  attributes: [262160, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  duration_ms: 20000
  power:
  - {type: RAGE, cost: 300}
  cooldown: {recovery_ms: 30000}
  shapeshift_mask: 65536
  levels: {base: 30, spell: 30}
  aura_options: {proc_chance: 100, proc_charges: 5, proc_type_mask: 20}
  class_options:
    set: 4
    mask: [0, 1048576, 0, 0]
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: DUMMY
    implicit_target: [UNIT_CASTER, NONE]
- id: 12723
  name: Sweeping Strikes
  skill_line: 26
  acquire_method: 3
  attributes: [262160, 0, 541065344, 512, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  range_yd: 100.0
  levels: {base: 1, spell: 1}
  effects:
  - index: 0
    effect: SCHOOL_DAMAGE
    base_points: 1.0
    chain_amplitude: 0.0
    implicit_target: [UNIT_TARGET_ENEMY, NONE]
- id: 12282
  name: Improved Heroic Strike
  skill_line: 26
  attributes: [262608, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  levels: {base: 1, spell: 1}
  class_options: {set: 4}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: ADD_FLAT_MODIFIER
    base_points: -10.0
    misc_value: [14, 0]
    spell_class_mask: [64, 0, 0, 0]
    implicit_target: [UNIT_CASTER, NONE]
- id: 12290
  name: Improved Overpower
  rank_text: Rank 1
  skill_line: 26
  attributes: [464, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  levels: {base: 1, spell: 1}
  class_options: {set: 4}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: ADD_FLAT_MODIFIER
    base_points: 25.0
    misc_value: [7, 0]
    spell_class_mask: [4, 0, 0, 0]
    implicit_target: [UNIT_CASTER, NONE]
- id: 16493
  name: Impale
  skill_line: 26
  attributes: [464, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  class_options: {set: 4}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: ADD_PCT_MODIFIER
    base_points: 10.0
    bonus_coefficient: 1.0
    misc_value: [15, 0]
    spell_class_mask: [3999288558, 33088, 1, 268436480]
    implicit_target: [UNIT_CASTER, NONE]
- id: 12862
  name: Improved Slam
  skill_line: 256
  attributes: [464, 0, 0, 0, 0, 0, 0, 0, 4096, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  levels: {base: 1, spell: 1}
  class_options: {set: 4}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: ADD_FLAT_MODIFIER
    base_points: -500.0
    misc_value: [10, 0]
    spell_class_mask: [2097152, 0, 0, 0]
    implicit_target: [UNIT_CASTER, NONE]
  - index: 1
    effect: APPLY_AURA
    aura: ADD_FLAT_MODIFIER
    base_points: -500.0
    misc_value: [21, 0]
    spell_class_mask: [2097152, 0, 0, 0]
    implicit_target: [UNIT_CASTER, NONE]
  - index: 2
    effect: APPLY_AURA
    aura: OVERRIDE_ACTIONBAR_SPELLS
    base_points: 1310197.0
    misc_value: [1464, 0]
    implicit_target: [UNIT_CASTER, NONE]
- id: 12163
  name: Two-Handed Weapon Specialization
  skill_line: 26
  attributes: [262352, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8192, 0]
  school_mask: 1
  levels: {base: 1, spell: 1}
  equipped_items: {class: 2, subclass_mask: 136546}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: MOD_DAMAGE_PERCENT_DONE
    base_points: 1.0
    misc_value: [1, 0]
    implicit_target: [UNIT_CASTER, NONE]
"#;

/// The overrides that go with [`SPELLS_YAML`] (copied from `data/spells/overrides/warrior.yaml`).
pub(crate) const OVERRIDES_YAML: &str = r#"
overrides:
  - id: 12834
    proc: { hit_mask: [CRITICAL] }
  - id: 12319
    proc: { hit_mask: [CRITICAL] }
    effects: [{ index: 0, script: TRIGGER_WITH_VALUE, params: { spell: 12966, effect: 0 } }]
  - id: 12162
    effects: [{ index: 0, script: DEEP_WOUNDS_BLEED, params: { duration_spell: 412609 } }]
  - id: 412609
    effects: [{ index: 0, script: NO_OP }]
  - id: 23881
    effects: [{ index: 1, script: ATTACK_POWER_PERCENT_DAMAGE }]
  - id: 20662
    effects: [{ index: 0, script: EXECUTE }]
  - id: 26651
    effects: [{ index: 0, script: NO_OP }]
  - id: 12296
    sim_flags: [START_OF_COMBAT]
    effects:
      - { index: 1, script: PERIODIC_RESOURCE_GAIN, params: { period_ms: 3000, resource: RAGE } }
      - { index: 2, script: NO_OP }
      - { index: 3, script: NO_OP }
  - id: 78
    threat: { flat: 145 }
  - id: 284
    threat: { flat: 145 }
  - id: 25288
    threat: { flat: 355 }
  - id: 1464
    sim_flags: [RESETS_SWING_TIMERS, STOPS_ATTACK_DURING_CAST, CANCELS_NEXT_SWING_QUEUE]
  - id: 1310197
    sim_flags: [STOPS_ATTACK_DURING_CAST, CANCELS_NEXT_SWING_QUEUE]
  - id: 12292
    sim_flags: [IGNORED]
  - id: 2457
    stance_passive: 21156
  - id: 2458
    stance_passive: 7381
  - id: 11585
    on_event: [{ source: MELEE_DODGE, script: ADD_COMBO_POINTS, params: { value: 1 } }]
  - id: 11597
    debuff_priority: high
"#;

/// The test spell db: the records above with their overrides.
pub(crate) fn db() -> SpellDb {
    db_with(|_| {})
}

/// The test spell db with `edit` applied to the records first.
pub(crate) fn db_with(edit: impl FnOnce(&mut SpellFile)) -> SpellDb {
    let mut file: SpellFile = serde_yaml::from_str(SPELLS_YAML).expect("valid spell yaml");
    edit(&mut file);
    let overrides: OverrideFile =
        serde_yaml::from_str(OVERRIDES_YAML).expect("valid override yaml");
    let mut db = SpellDb::new();
    db.add_file(file).expect("valid records");
    let mut all = Overrides::new();
    all.add_file(overrides).expect("valid overrides");
    db.set_overrides(all);
    db.check_references().expect("consistent references");
    db
}

/// A one-character world around a [`CharacterSpells`] registry.
pub(crate) struct World {
    pub db: SpellDb,
    pub engine: Engine,
    pub target: Target,
    pub stats: CharacterStats,
    pub spells: CharacterSpells,
    pub raid: SharedBuffRegistry,
    pub modifiers: SpellModifiers,
    pub level: u32,
    pub rage: u32,
    pub combo_points: u32,
    pub stance: Stance,
    pub next_gcd: f64,
    pub next_stance_cd: f64,
    /// Forces `cast_in_progress` regardless of the registry.
    pub casting: bool,
    pub rolls: VecDeque<PhysicalAttackResult>,
    pub extra_crits: Vec<u32>,
    pub can_crits: Vec<bool>,
    pub combat_length: f64,
    pub armor: i32,
    pub block_value: u32,
    pub aura_log: Vec<String>,
    pub ticks: Vec<TickReport>,
    pub casting_speed_mod: f64,
    pub attack_log: Vec<&'static str>,
    pub completed_casts: Vec<CastReport>,
    /// Whether every `SpellEquippedItems` requirement is met.
    pub weapon_ok: bool,
    pub caster_states: Vec<AuraState>,
    pub target_states: Vec<AuraState>,
    pub mh_speed: Option<f64>,
    pub oh_speed: Option<f64>,
    /// `(spell, trigger value)` of every `trigger_spell` call.
    pub trigger_log: Vec<(u32, Option<f64>)>,
    pub extra_attacks: u32,
    pub stance_rage_retained: i32,
    pub offhand_damage_percent: i32,
    pub offhand_rage_percent: i32,
    /// `OFFHAND_COPY` abilities, once per active aura.
    pub offhand_copies: Vec<u32>,
    pub actionbar_log: Vec<(u32, u32, bool)>,
}

impl World {
    pub fn new() -> Self {
        Self::with_db(db())
    }

    pub fn with_db(db: SpellDb) -> Self {
        let mut engine = Engine::new();
        engine.prepare_iteration(0.0);
        World {
            db,
            engine,
            target: Target::new(63),
            stats: CharacterStats::new(),
            spells: CharacterSpells::new(CharId(0), 1),
            raid: SharedBuffRegistry::new(),
            modifiers: SpellModifiers::new(),
            level: 60,
            rage: 100,
            combo_points: 0,
            stance: Stance::Battle,
            next_gcd: 0.0,
            next_stance_cd: 0.0,
            casting: false,
            rolls: VecDeque::new(),
            extra_crits: Vec::new(),
            can_crits: Vec::new(),
            combat_length: 300.0,
            armor: 0,
            block_value: 0,
            aura_log: Vec::new(),
            ticks: Vec::new(),
            casting_speed_mod: 1.0,
            attack_log: Vec::new(),
            completed_casts: Vec::new(),
            weapon_ok: true,
            caster_states: Vec::new(),
            target_states: Vec::new(),
            mh_speed: Some(2.6),
            oh_speed: Some(1.8),
            trigger_log: Vec::new(),
            extra_attacks: 0,
            stance_rage_retained: 0,
            offhand_damage_percent: 0,
            offhand_rage_percent: 0,
            offhand_copies: Vec::new(),
            actionbar_log: Vec::new(),
        }
    }

    /// Adds spell `id` from the db (not enabled).
    pub fn add(&mut self, id: u32) -> AddedSpell {
        let db = std::mem::take(&mut self.db);
        let added = self.spells.add_spell(&db, id, 0, &mut self.raid);
        self.db = db;
        added
    }

    /// Adds and enables spell `id`; returns its handle (procs are enabled through the procs).
    pub fn learn(&mut self, id: u32) -> AddedSpell {
        let added = self.add(id);
        if let Some(spell) = added.spell {
            self.with_spell(spell, |spell, world| spell.enable(world));
        }
        if let Some(proc) = added.proc {
            let mut procs = self.spells.take_procs();
            procs.enable(proc, self);
            self.spells.put_procs(procs);
        }
        added
    }

    pub fn spell_id(&self, game_id: u32) -> SpellId {
        self.spells
            .spell_by_game_id(game_id)
            .unwrap_or_else(|| panic!("spell {game_id} not added"))
    }

    pub fn spell(&self, game_id: u32) -> &Spell {
        self.spells.spell(self.spell_id(game_id))
    }

    /// Runs `f` with the spell taken out of the registry.
    pub fn with_spell<R>(&mut self, id: SpellId, f: impl FnOnce(&mut Spell, &mut World) -> R) -> R {
        let mut spell = self.spells.take_spell(id);
        let result = f(&mut spell, self);
        self.spells.put_spell(id, spell);
        result
    }

    pub fn status(&self, game_id: u32) -> SpellStatus {
        self.spell(game_id).status(self)
    }

    pub fn perform(&mut self, game_id: u32) -> CastReport {
        let id = self.spell_id(game_id);
        self.with_spell(id, |spell, world| spell.perform(world))
    }

    pub fn run_proc_check(&mut self, source: ProcSource) -> Vec<(crate::ids::ProcId, CastReport)> {
        let mut procs = self.spells.take_procs();
        let reports = procs.run_proc_check(source, self);
        self.spells.put_procs(procs);
        reports
    }

    /// Uses a charge of every buff that reacts to `source`.
    pub fn consume_charges(&mut self, source: ProcSource) {
        for id in self.spells.charge_consumers(source) {
            let (buff, mut ctx) = self.buff_ctx(id);
            if buff.use_charge(&mut ctx) == ChargeUse::Removed {
                self.remove_auras(id);
            }
        }
    }

    pub fn advance_to(&mut self, time: f64) {
        self.engine.add_event(Event::new(
            time,
            EventKind::EncounterStart {
                character: CharId(0),
            },
        ));
        while let Some(event) = self.engine.next_event() {
            if event.time >= time {
                break;
            }
        }
    }

    /// Dispatches events up to and including `until`: buff removals, dot ticks and cast
    /// completions of the registry's spells.
    pub fn run(&mut self, until: f64) {
        self.engine
            .add_event(Event::new(until, EventKind::EncounterEnd));
        while let Some(event) = self.engine.next_event() {
            match event.kind {
                EventKind::EncounterEnd => break,
                EventKind::BuffRemoval {
                    buff, iteration, ..
                } => {
                    let stacks = self.buff_ref(buff).stacks();
                    let (b, mut ctx) = self.buff_ctx(buff);
                    if b.remove(iteration, &mut ctx) {
                        for _ in 0..stacks.max(1) {
                            self.remove_auras(buff);
                        }
                    }
                }
                EventKind::DotTick {
                    spell,
                    application_id,
                    ..
                } => {
                    if let Some(tick) =
                        self.with_spell(spell, |s, world| s.perform_periodic(application_id, world))
                    {
                        self.ticks.push(tick);
                    }
                }
                EventKind::CastComplete { spell, cast_id, .. } => {
                    if let Some(report) =
                        self.with_spell(spell, |s, world| s.complete_cast(cast_id, world))
                    {
                        self.completed_casts.push(report);
                    }
                }
                _ => {}
            }
        }
    }

    fn buff_ref(&self, id: BuffId) -> &Buff {
        match self.spells.buff_slot(id) {
            BuffSlot::Owned(buff) => buff,
            BuffSlot::Shared(handle) => self.raid.shared_buff(*handle),
        }
    }

    fn buff_ctx(&mut self, id: BuffId) -> (&mut Buff, BuffContext<'_>) {
        let buff = match self.spells.buff_slot_mut(id) {
            BuffSlot::Owned(buff) => buff,
            BuffSlot::Shared(handle) => self.raid.shared_buff_mut(*handle),
        };
        (
            buff,
            BuffContext {
                engine: &mut self.engine,
                target: &mut self.target,
                character: CharId(0),
                buff: id,
            },
        )
    }

    fn apply_auras(&mut self, id: BuffId) {
        let effects = self.buff_ref(id).effects.clone();
        for effect in &effects {
            self.aura_log.push(format!("+{:?}", effect.aura()));
            effect.apply_aura(self, effect.record().targets_enemy());
        }
    }

    fn remove_auras(&mut self, id: BuffId) {
        let effects = self.buff_ref(id).effects.clone();
        for effect in &effects {
            self.aura_log.push(format!("-{:?}", effect.aura()));
            effect.remove_aura(self, effect.record().targets_enemy());
        }
    }
}

impl EffectHost for World {
    fn caster_level(&self) -> u32 {
        self.level
    }
    fn combo_points(&self) -> u32 {
        self.combo_points
    }
    fn gain_combo_points(&mut self, amount: u32) {
        self.combo_points = (self.combo_points + amount).min(5);
    }
    fn spend_combo_points(&mut self) {
        self.combo_points = 0;
    }
    fn resource_level(&self, _: ResourceType) -> u32 {
        self.rage
    }
    fn gain_resource(&mut self, _: ResourceType, amount: u32) -> u32 {
        let before = self.rage;
        self.rage = (self.rage + amount).min(100);
        self.rage - before
    }
    fn melee_ap(&self) -> u32 {
        1000
    }
    fn random_in_range(&mut self, min: f64, max: f64) -> f64 {
        (min + max) / 2.0
    }
    fn random_normalized_mh_dmg(&mut self) -> f64 {
        300.0
    }
    fn random_non_normalized_mh_dmg(&mut self) -> f64 {
        400.0
    }
    fn roll_melee_ability(
        &mut self,
        _: IncludedOutcomes,
        extra_crit: u32,
        can_crit: bool,
    ) -> PhysicalAttackResult {
        self.extra_crits.push(extra_crit);
        self.can_crits.push(can_crit);
        self.rolls.pop_front().expect("no roll queued")
    }
    fn stats_mut(&mut self) -> &mut CharacterStats {
        &mut self.stats
    }
    fn target_mut(&mut self) -> &mut Target {
        &mut self.target
    }
    fn increase_melee_attack_speed(&mut self, percent: u32) {
        self.stats.increase_melee_attack_speed(percent);
    }
    fn decrease_melee_attack_speed(&mut self, percent: u32) {
        self.stats.decrease_melee_attack_speed(percent);
    }
    fn swap_stance(&mut self, stance: Stance) {
        self.stance = stance;
    }
    fn spell_modifiers(&self) -> &SpellModifiers {
        &self.modifiers
    }
    fn spell_modifiers_mut(&mut self) -> &mut SpellModifiers {
        &mut self.modifiers
    }
    fn add_extra_attacks(&mut self, count: u32) {
        self.extra_attacks += count;
    }
    fn adjust_stance_rage_retained(&mut self, delta: i32) {
        self.stance_rage_retained += delta;
    }
    fn adjust_offhand_damage_percent(&mut self, percent: i32) {
        self.offhand_damage_percent += percent;
    }
    fn adjust_offhand_rage_percent(&mut self, percent: i32) {
        self.offhand_rage_percent += percent;
    }
    fn adjust_offhand_copy(&mut self, spell: u32, apply: bool) {
        if apply {
            self.offhand_copies.push(spell);
        } else if let Some(i) = self.offhand_copies.iter().position(|&s| s == spell) {
            self.offhand_copies.remove(i);
        }
    }
    fn override_actionbar_spell(&mut self, replaced: u32, replacement: u32, apply: bool) {
        self.actionbar_log.push((replaced, replacement, apply));
        self.spells
            .apply_actionbar_override(replaced, replacement, apply);
    }
}

impl SpellHost for World {
    fn character_id(&self) -> CharId {
        CharId(0)
    }
    fn resource_type(&self) -> ResourceType {
        ResourceType::Rage
    }
    fn engine(&self) -> &Engine {
        &self.engine
    }
    fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }
    fn combat_length(&self) -> f64 {
        self.combat_length
    }
    fn on_global_cooldown(&self) -> bool {
        self.engine.current_time() < self.next_gcd
    }
    fn global_cooldown(&self) -> f64 {
        1.5
    }
    fn start_global_cooldown(&mut self) {
        self.next_gcd = self.engine.current_time() + 1.5;
    }
    fn on_stance_cooldown(&self) -> bool {
        self.engine.current_time() < self.next_stance_cd
    }
    fn start_stance_cooldown(&mut self) {
        self.next_stance_cd = self.engine.current_time() + 1.0;
    }
    fn cast_in_progress(&self) -> bool {
        self.casting || self.spells.cast_in_progress()
    }
    fn start_cast(&mut self) -> u32 {
        self.spells.start_cast()
    }
    fn complete_cast(&mut self, cast_id: u32) {
        self.spells.complete_cast(cast_id);
    }
    fn casting_speed_mod(&self) -> f64 {
        self.casting_speed_mod
    }
    fn casting_speed_flat_reduction(&self) -> u32 {
        0
    }
    fn stop_attack(&mut self) {
        self.attack_log.push("stop");
        self.spells.stop_attack();
    }
    fn start_attack(&mut self) {
        self.attack_log.push("start");
        self.spells.start_attack();
    }
    fn reset_swing_timers(&mut self) {
        self.attack_log.push("reset");
    }
    fn queue_next_swing(&mut self, spell: SpellId) {
        self.attack_log.push("queue");
        self.spells.queue_next_swing(spell);
    }
    fn cancel_next_swing(&mut self) {
        if self.spells.cancel_next_swing().is_some() {
            self.attack_log.push("unqueue");
        }
    }
    fn queued_next_swing(&self) -> Option<SpellId> {
        self.spells.queued_next_swing()
    }
    fn stance(&self) -> Stance {
        self.stance
    }
    fn equipped_item_matches(&self, _: &EquippedItems) -> bool {
        self.weapon_ok
    }
    fn caster_aura_state(&self, state: AuraState) -> bool {
        self.caster_states.contains(&state)
    }
    fn target_aura_state(&self, state: AuraState) -> bool {
        self.target_states.contains(&state)
    }
    fn aura_active(&self, spell: u32) -> bool {
        self.spells
            .buff_ids()
            .map(|id| self.buff_ref(id))
            .any(|buff| buff.spell() == spell && buff.is_active())
    }
    fn lose_resource(&mut self, _: ResourceType, amount: u32) {
        self.rage -= amount;
    }
    /// Whole rage only: the refund rounds.
    fn refund_resource(&mut self, _: ResourceType, amount: f64) {
        self.rage = (self.rage + amount.round() as u32).min(100);
    }
    fn cooldown(&self, id: CooldownId) -> &CooldownControl {
        self.spells.cooldowns().get(id)
    }
    fn cooldown_mut(&mut self, id: CooldownId) -> &mut CooldownControl {
        self.spells.cooldowns_mut().get_mut(id)
    }
    fn buff(&self, id: BuffId) -> &Buff {
        self.buff_ref(id)
    }
    fn buff_mut(&mut self, id: BuffId) -> &mut Buff {
        self.buff_ctx(id).0
    }
    /// Aura effects are applied once per stack (Sunder Armor's −450 armor × 5).
    fn apply_buff(&mut self, id: BuffId) -> BuffApplication {
        let before = self.buff_ref(id).stacks();
        let (buff, mut ctx) = self.buff_ctx(id);
        let application = buff.apply(&mut ctx);
        match application {
            BuffApplication::Applied { .. } => self.apply_auras(id),
            BuffApplication::Refreshed { stacks } if stacks > before => self.apply_auras(id),
            _ => {}
        }
        application
    }
    fn cancel_buff(&mut self, id: BuffId) -> bool {
        let stacks = self.buff_ref(id).stacks();
        let (buff, mut ctx) = self.buff_ctx(id);
        let cancelled = buff.cancel(&mut ctx);
        if cancelled {
            for _ in 0..stacks.max(1) {
                self.remove_auras(id);
            }
        }
        cancelled
    }
    fn enable_buff(&mut self, id: BuffId) {
        if self.spells.owned_buff(id).is_some() {
            self.spells.enable_buff(id);
        } else {
            let buff = self.buff_ctx(id).0;
            buff.set_instance_id(InstanceId(id.0 + 100));
            buff.enable();
        }
    }
    fn disable_buff(&mut self, id: BuffId) {
        self.spells.disable_buff(id);
    }
    fn trigger_spell(&mut self, spell: u32, trigger_value: Option<f64>) -> Option<CastReport> {
        self.trigger_log.push((spell, trigger_value));
        let id = self.spells.spell_by_game_id(spell)?;
        Some(self.with_spell(id, |s, world| {
            s.set_trigger_value(trigger_value);
            s.perform_triggered(world)
        }))
    }
    fn set_spell_effect_value(&mut self, spell: u32, index: u32, value: f64) {
        if let Some(id) = self.spells.spell_by_game_id(spell) {
            self.with_spell(id, |s, world| s.set_effect_value(world, index, value));
        } else if let Some(id) = self.spells.proc_by_game_id(spell) {
            let mut procs = self.spells.take_procs();
            procs
                .get_mut(id)
                .spell_mut()
                .set_effect_value(self, index, value);
            self.spells.put_procs(procs);
        }
    }
    fn target_armor(&self) -> i32 {
        self.armor
    }
    fn target_block_value(&self) -> u32 {
        self.block_value
    }
    fn total_physical_damage_mod(&self) -> f64 {
        1.0
    }
    fn flat_physical_damage_bonus(&self) -> u32 {
        0
    }
    fn melee_ability_crit_dmg_mod(&self) -> f64 {
        2.0
    }
    fn total_threat_mod(&self) -> f64 {
        self.stats.get_total_threat_mod()
    }
    fn avg_mh_weapon_damage(&self) -> f64 {
        200.0
    }
    fn offhand_copy_active(&self, spell: u32) -> bool {
        self.offhand_copies.contains(&spell) && self.oh_speed.is_some()
    }
    fn roll_offhand_melee_ability(
        &mut self,
        _: IncludedOutcomes,
        extra_crit: u32,
        can_crit: bool,
    ) -> PhysicalAttackResult {
        self.extra_crits.push(extra_crit);
        self.can_crits.push(can_crit);
        self.rolls.pop_front().expect("no roll queued")
    }
    /// Half the main-hand values, so an off-hand strike is told apart.
    fn random_oh_weapon_dmg(&mut self, normalized: bool) -> f64 {
        if normalized {
            150.0
        } else {
            200.0
        }
    }
    fn offhand_penalty(&self) -> f64 {
        0.5
    }
}

impl ProcHost for World {
    fn base_weapon_speed(&self, hand: Hand) -> Option<f64> {
        match hand {
            Hand::Mainhand => self.mh_speed,
            Hand::Offhand => self.oh_speed,
        }
    }
}
