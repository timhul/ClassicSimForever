//! `Crit-aura-suppression.md`: against +3 level mobs a flat modifier reduces the crit gained
//! from auras (talents, `Equip:` crit on gear, buffs, consumables — not crit from agility) by
//! ~1.8 %, on top of the 3 % suppression from the skill / defense difference.
//!
//! Crit values are in hundredths of a percent, as the engine stores them.

use std::sync::Arc;

use crate::character::tests::{race, warrior_class, Fixture};
use crate::character::{Character, SimParams};
use crate::combat_roll::CombatRoll;
use crate::ids::CharId;
use crate::item::{EquipmentDb, EquipmentSlot, ItemSpec};
use crate::mechanics::Mechanics;
use crate::phase::Phase;
use crate::race::Race;
use crate::spell::dbc::{AuraType, ImplicitTarget, SpellAttr0, SpellEffectName};
use crate::spell::record::{EffectRecord, SpellRecord};
use crate::spell::SpellResult;

/// The 3 % from `(300 − 315) × 0.2 %`.
const SKILL_SUPPRESSION: u32 = 300;
/// The flat modifier on crit from auras.
const AURA_SUPPRESSION: u32 = 180;

/// "the modifier is most likely a flat 1.8% reduction to your crit chance gained from auras"
#[test]
fn aura_crit_loses_a_flat_1_8_percent_against_plus_three_mobs() {
    let mechanics = Mechanics::new(63);
    for aura_crit in [200, 300, 500, 600, 800, 1000, 1200, 1300, 1400, 1700] {
        assert_eq!(
            mechanics.suppressed_aura_crit_chance(60, aura_crit),
            aura_crit - AURA_SUPPRESSION,
            "{} % aura crit",
            f64::from(aura_crit) / 100.0
        );
    }
}

/// The 1 % aura crit tests "seem to be suppressed entirely".
#[test]
fn one_percent_of_aura_crit_is_suppressed_entirely() {
    assert_eq!(Mechanics::new(63).suppressed_aura_crit_chance(60, 100), 0);
}

/// "without any crit aura from talents/gear/buffs/etc. you only suffer the 3% suppression due to
/// weapon skill / defense difference"
#[test]
fn without_aura_crit_only_the_skill_suppression_applies() {
    assert_eq!(Mechanics::new(63).suppressed_aura_crit_chance(60, 0), 0);
    // magey's 7.68 % base crit vs +3: 4.68 %.
    assert_eq!(
        CombatRoll::new(63).get_suppressed_crit(60, 768),
        768 - SKILL_SUPPRESSION
    );
}

/// "putting your total suppression against +3 level mobs at 4.8%"
#[test]
fn total_suppression_is_4_8_percent_with_at_least_1_8_percent_aura_crit() {
    let mechanics = Mechanics::new(63);
    let roll = CombatRoll::new(63);
    for (base, aura) in [(841, 500), (523, 1200), (910, 1400), (334, 500)] {
        let effective =
            roll.get_suppressed_crit(60, base + mechanics.suppressed_aura_crit_chance(60, aura));
        assert_eq!(
            effective,
            base + aura - 480,
            "{} % base + {} % aura crit",
            f64::from(base) / 100.0,
            f64::from(aura) / 100.0
        );
    }
}

/// The measured crit rates of the wiki's table: `base + aura − 1.8 % − 3 %` (with the 1 % aura
/// rows suppressed to 0) must fall inside the measured 95 % confidence interval.
#[test]
fn measured_crit_rates_match_the_1_8_percent_model() {
    // (player, class, base crit, aura crit, measured crit, ±95 % CI), all vs +3 level mobs.
    let rows = [
        ("magey", "Rogue", 768, 100, 497, 57),
        ("theta", "Rogue", 1223, 100, 927, 74),
        ("magey", "Rogue", 768, 200, 421, 81),
        ("Pyte", "Warrior", 523, 200, 240, 43),
        ("Toraque", "Warrior", 841, 300, 634, 67),
        ("Toraque", "Warrior", 841, 500, 843, 105),
        ("dmzor", "Paladin", 433, 500, 451, 50),
        ("theta (+10 skill)", "Rogue", 1279, 500, 1312, 118),
        ("WatchYourSixx", "Hunter", 334, 500, 332, 39),
        ("dmzor", "Druid", 803, 600, 962, 78),
        ("Toraque", "Warrior", 841, 800, 1191, 89),
        ("ShadowPanther", "Rogue", 739, 1000, 1230, 109),
        ("Pyte (+5 skill)", "Warrior", 523, 1200, 1261, 118),
        ("Toraque", "Warrior", 819, 1300, 1678, 115),
        ("Puffymuffins (+5 skill)", "Warrior", 910, 1400, 1780, 128),
        ("Toraque", "Warrior", 841, 1700, 2050, 141),
    ];
    let mechanics = Mechanics::new(63);
    let roll = CombatRoll::new(63);
    let mut failures = Vec::new();
    for (player, class, base, aura, measured, ci) in rows {
        let predicted =
            roll.get_suppressed_crit(60, base + mechanics.suppressed_aura_crit_chance(60, aura));
        let (low, high) = (measured - ci, measured + ci);
        if !(low..=high).contains(&predicted) {
            failures.push(format!(
                "{player} ({class}, {:.2} % base + {:.2} % aura): simulator {:.2} %, measured {:.2} % ±{:.2}",
                f64::from(base) / 100.0,
                f64::from(aura) / 100.0,
                f64::from(predicted) / 100.0,
                f64::from(measured) / 100.0,
                f64::from(ci) / 100.0
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "crit aura suppression:\n{}",
        failures.join("\n")
    );
}

// ------------------------------------------------------------------ through the character

const SWORD: u32 = 1;
const CRIT_RING: u32 = 2;
const CRUELTY: u32 = 12320;
const MONGOOSE: u32 = 17538;

const ITEMS_YAML: &str = r#"
- id: 1
  name: Sword
  phase: 1
  slot: "1H"
  type: SWORD
  quality: EPIC
  req_lvl: 60
  item_lvl: 60
  damage: { min: 100, max: 100, speed: 2.6 }
- id: 2
  name: Ring of Striking
  phase: 1
  slot: RING
  type: RING
  quality: EPIC
  req_lvl: 60
  item_lvl: 60
  stats: { CRIT_CHANCE: 0.02 }
"#;

fn passive_aura(id: u32, name: &str, aura: AuraType, points: f32) -> SpellRecord {
    let mut record = SpellRecord::new(id, name);
    record.attributes[0] = SpellAttr0::PASSIVE.bits();
    let mut effect = EffectRecord::new(0, SpellEffectName::ApplyAura);
    effect.aura = aura;
    effect.base_points = points;
    effect.implicit_target = [ImplicitTarget::UnitCaster, ImplicitTarget::None];
    record.effects.push(effect);
    record
}

/// A fixture whose item db carries a fixed-damage sword and a +2 % crit ring.
fn fixture() -> Fixture {
    let mut f = Fixture::orc_warrior();
    let items: Vec<ItemSpec> = serde_yaml::from_str(ITEMS_YAML).unwrap();
    let equipment = Arc::new(EquipmentDb::from_specs(items, Vec::new()).unwrap());
    f.character = Character::new(
        CharId(0),
        warrior_class(),
        &race(Race::Orc),
        equipment,
        Phase::MoltenCore,
        SimParams::default(),
        63,
        0,
        0,
    );
    // Cruelty (talent): +5 % melee crit through the aura interpreter.
    f.db.add(
        None,
        passive_aura(CRUELTY, "Cruelty", AuraType::ModWeaponCritPercent, 5.0),
    )
    .unwrap();
    // Elixir of the Mongoose (consumable): +25 agility and +2 % crit for an hour.
    let mut mongoose = SpellRecord::new(MONGOOSE, "Elixir of the Mongoose");
    mongoose.class_mask = 1;
    mongoose.duration_ms = Some(3_600_000);
    let mut crit = EffectRecord::new(0, SpellEffectName::ApplyAura);
    crit.aura = AuraType::ModWeaponCritPercent;
    crit.base_points = 2.0;
    crit.implicit_target = [ImplicitTarget::UnitCaster, ImplicitTarget::None];
    let mut agility = EffectRecord::new(1, SpellEffectName::ApplyAura);
    agility.aura = AuraType::ModStat;
    agility.base_points = 25.0;
    agility.misc_value = [1, 0];
    agility.implicit_target = [ImplicitTarget::UnitCaster, ImplicitTarget::None];
    mongoose.effects.push(crit);
    mongoose.effects.push(agility);
    f.db.add(None, mongoose).unwrap();
    f
}

fn mh_crit(f: &Fixture) -> u32 {
    let view = f.target.stat_view();
    let ctx = f.character.stat_context(&view);
    f.character.stats().get_mh_crit_chance(&ctx)
}

/// Crit from talents (Cruelty), gear (`Equip:` crit) and consumables (Elixir of the Mongoose)
/// is aura crit and loses the flat 1.8 %; crit from agility — base or from the elixir — is
/// not; the roll then takes the 3 % skill suppression. Against an equal-level mob nothing is
/// suppressed.
#[test]
fn crit_from_talents_gear_and_consumables_is_suppressed_but_agility_crit_is_not() {
    let mut f = fixture();
    f.equip(EquipmentSlot::Mainhand, SWORD);
    // 2 % class base + 77 agility / 20.
    assert_eq!(mh_crit(&f), 200 + 385, "no aura crit yet");

    let cruelty = f.learn(CRUELTY);
    f.ctx().enable_spell(cruelty);
    assert_eq!(
        mh_crit(&f),
        200 + 385 + (500 - AURA_SUPPRESSION),
        "Cruelty's 5 % loses 1.8 %"
    );

    f.equip(EquipmentSlot::Ring1, CRIT_RING);
    assert_eq!(
        mh_crit(&f),
        200 + 385 + (700 - AURA_SUPPRESSION),
        "the ring's 2 % joins the aura crit"
    );

    let mongoose = f.learn(MONGOOSE);
    let report = f.ctx().cast(mongoose);
    assert_eq!(report.result, SpellResult::Success);
    // (77 + 25) agility / 20 = 5.1 %, unsuppressed; 5 + 2 + 2 = 9 % aura crit.
    assert_eq!(
        mh_crit(&f),
        200 + 510 + (900 - AURA_SUPPRESSION),
        "the elixir's agility is not suppressed, its crit is"
    );

    // The roll applies the skill / defense suppression: 4.8 % in total.
    let crit = mh_crit(&f);
    assert_eq!(
        f.character.roll_mut().get_suppressed_crit(60, crit),
        200 + 510 + 900 - 480,
        "total suppression vs +3"
    );

    // Against an equal-level mob nothing is suppressed.
    f.character.roll_mut().set_target_level(60);
    f.target.set_level(60);
    assert_eq!(mh_crit(&f), 200 + 510 + 900, "no aura suppression vs +0");
    let crit = mh_crit(&f);
    assert_eq!(
        f.character.roll_mut().get_suppressed_crit(60, crit),
        200 + 510 + 900,
        "no skill suppression vs +0"
    );
}
