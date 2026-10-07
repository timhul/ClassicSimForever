//! The Paladin's mana at runtime: the 2 second regeneration ticks under the five-second rule,
//! Reverence, costs in percent of base mana and the cost talents, the mana consumables, the
//! `resource_percent` variable, and a fight that runs dry.

use super::energy::{data, pull};
use super::paladin::{paladin, with_talents};
use super::rogue::highest_rank;
use super::*;
use crate::buff::external::ExternalBuffDb;
use crate::rotation::BuiltinVariable;
use crate::rotation::condition::ConditionContext;
use crate::spell::SpellResult;

const REVERENCE: u32 = 110871;
const BENEDICTION: u32 = 105706;
const HOLY_CONDUIT: u32 = 105704;
const SEAL_OF_COMMAND_TALENT: u32 = 105696;
const TWIST_OF_LIGHT: u32 = 105692;
const IMPROVED_SEAL_OF_FURY: u32 = 110875;
const SWIFT_JUDGEMENT: u32 = 110878;

const JUDGEMENT: u32 = 20271;
const HOLY_STRIKE: u32 = 10333;
const SEAL_OF_COMMAND: u32 = 20920;
const CONSECRATION: u32 = 20924;
const SWIFT_JUDGEMENT_SPELL: u32 = 1310994;

fn mana(f: &Fixture) -> u32 {
    f.character
        .resource_level(ResourceType::Mana, f.engine.current_time())
}

fn cost(f: &mut Fixture, game_id: u32) -> u32 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).resource_cost(&ctx)
}

fn status(f: &mut Fixture, game_id: u32) -> SpellStatus {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).status(&ctx)
}

/// Pulls at 0 and spends `amount` mana right after, which starts the five-second rule.
fn pulled_after_spending(mut f: Fixture, amount: u32) -> Fixture {
    pull(&mut f);
    f.advance_to(0.01);
    f.character.resource_mut().lose(amount, 0.01);
    f
}

/// An Undead Paladin (80 spirit, no gear): 15 + 80 / 5 = 31 mana per 5 seconds from spirit,
/// 12.4 a tick. Nothing inside the five-second rule (no mp5 gear), then 12, 12, 13 as the
/// fraction carries over.
#[test]
fn regeneration_ticks_every_2_seconds_outside_the_five_second_rule() {
    let mut f = pulled_after_spending(paladin(Race::Undead, &[]), 1000);
    let spent = mana(&f);
    f.advance_to(5.9);
    assert_eq!(mana(&f), spent, "the ticks at 2 and 4 fall inside the rule");
    let mut gains = Vec::new();
    for t in [6.1, 8.1, 10.1] {
        let before = mana(&f);
        f.advance_to(t);
        gains.push(mana(&f) - before);
    }
    assert_eq!(gains, [12, 12, 13]);
    let regenerated = f
        .character
        .statistics
        .resource_statistics(crate::character::context::REGENERATION, 1)
        .unwrap()
        .gain(ResourceType::Mana);
    assert_eq!(regenerated, 37.0);
}

/// Reverence 3/3: 30 % of the spirit regeneration continues inside the rule (3.72 a tick).
#[test]
fn reverence_keeps_30_percent_inside_the_five_second_rule() {
    let base = with_talents(&[(REVERENCE, 3)]);
    // A Human: 55 + 22 spirit, The Human Spirit +5 % = 81; 15 + 81 / 5 = 31.2 mp5 = 12.48 a tick.
    let mut f = pulled_after_spending(base, 1000);
    let mut gains = Vec::new();
    for t in [2.1, 4.1, 6.1] {
        let before = mana(&f);
        f.advance_to(t);
        gains.push(mana(&f) - before);
    }
    // 3.744 → 3 (0.744), 4.488 → 4 (0.488), then outside: 12.968 → 12.
    assert_eq!(gains, [3, 4, 12]);
}

/// A full mana bar does not grow, and the overflow counts as lost.
#[test]
fn regeneration_at_the_cap_is_lost() {
    let mut f = paladin(Race::Undead, &[]);
    pull(&mut f);
    f.advance_to(4.1);
    assert_eq!(mana(&f), f.character.max_resource_level(ResourceType::Mana));
    assert_eq!(f.character.statistics.lost_at_cap(ResourceType::Mana), 24.0);
}

/// Judgement costs 6 % of the base mana: 1512 × 6 % = 90.72, 90.
#[test]
fn judgement_costs_six_percent_of_base_mana() {
    let mut f = paladin(Race::Human, &[]);
    assert_eq!(cost(&mut f, JUDGEMENT), 90);
    assert_eq!(cost(&mut f, HOLY_STRIKE), 20);
    assert_eq!(cost(&mut f, CONSECRATION), 565);
}

/// Benediction −10 % on instants, Holy Conduit −40 % on Consecration, Twist of Light −20 % on
/// the seals; the percentages add up.
#[test]
fn cost_talents() {
    let mut f = with_talents(&[(SEAL_OF_COMMAND_TALENT, 1)]);
    assert_eq!(cost(&mut f, SEAL_OF_COMMAND), 210);
    let mut f = with_talents(&[(SEAL_OF_COMMAND_TALENT, 1), (BENEDICTION, 5)]);
    assert_eq!(cost(&mut f, SEAL_OF_COMMAND), 189);
    assert_eq!(cost(&mut f, HOLY_STRIKE), 18);
    let mut f = with_talents(&[(HOLY_CONDUIT, 2)]);
    assert_eq!(cost(&mut f, CONSECRATION), 339);
    let mut f = with_talents(&[
        (SEAL_OF_COMMAND_TALENT, 1),
        (BENEDICTION, 5),
        (TWIST_OF_LIGHT, 1),
    ]);
    assert_eq!(cost(&mut f, SEAL_OF_COMMAND), 147);
}

/// Swift Judgement makes the next Judgement free.
#[test]
fn swift_judgement_makes_the_next_judgement_free() {
    let mut f = with_talents(&[(IMPROVED_SEAL_OF_FURY, 1), (SWIFT_JUDGEMENT, 1)]);
    pull(&mut f);
    f.advance_to(0.01);
    assert_eq!(cost(&mut f, JUDGEMENT), 90);
    let id = f.spell_id(SWIFT_JUDGEMENT_SPELL);
    assert_eq!(f.ctx().cast(id).result, SpellResult::Success);
    assert_eq!(cost(&mut f, JUDGEMENT), 0);
}

/// Without the mana a spell is not castable.
#[test]
fn a_spell_needs_its_mana() {
    let mut f = paladin(Race::Human, &[]);
    pull(&mut f);
    f.advance_to(0.01);
    assert_eq!(status(&mut f, HOLY_STRIKE), SpellStatus::Available);
    let left = mana(&f);
    f.character.resource_mut().lose(left - 19, 0.01);
    assert_eq!(
        status(&mut f, HOLY_STRIKE),
        SpellStatus::InsufficientResources
    );
}

/// `resource_percent` reads the mana as a percent of the maximum.
#[test]
fn resource_percent_reads_the_mana() {
    let mut f = paladin(Race::Undead, &[]);
    pull(&mut f);
    f.advance_to(0.01);
    assert_eq!(f.ctx().variable(BuiltinVariable::ResourcePercent), 100.0);
    let max = f.character.max_resource_level(ResourceType::Mana);
    f.character.resource_mut().lose(max / 2, 0.01);
    assert_eq!(f.ctx().variable(BuiltinVariable::ResourcePercent), 50.0);
}

/// The mana consumables: Major Mana Potion 1350-2250 on the potion category, Demonic Rune
/// 900-1500 (its damage to the user is not simulated) on the rune category, which it shares
/// with Night Dragon's Breath.
#[test]
fn mana_consumables_restore_mana_on_their_categories() {
    let registry = ExternalBuffDb::load(&data().join("external_buffs.yaml")).unwrap();
    let mut f = paladin(Race::Undead, &[]);
    let consumables = ["Major Mana Potion", "Demonic Rune", "Night Dragon's Breath"]
        .iter()
        .map(|name| registry.consumable(name).unwrap().clone())
        .collect();
    let db = std::mem::take(&mut f.db);
    f.ctx().set_consumables(&db, consumables);
    f.db = db;
    f.ctx().prepare_set_of_combat_iterations();
    let mut f = pulled_after_spending(f, 2000);

    let use_now = |f: &mut Fixture, name: &str| {
        let id = highest_rank(f, name);
        let ctx = f.ctx();
        let status = ctx.character.spells().spell(id).status(&ctx);
        (
            status,
            (status == SpellStatus::Available).then(|| f.ctx().cast(id).result),
        )
    };
    let before = mana(&f);
    let (_, result) = use_now(&mut f, "Major Mana Potion");
    assert_eq!(result, Some(SpellResult::Success));
    let gained = mana(&f) - before;
    assert!((1350..=2250).contains(&gained), "{gained}");

    let spend = mana(&f) - 100;
    f.character.resource_mut().lose(spend, 0.01);
    let before = mana(&f);
    let (_, result) = use_now(&mut f, "Demonic Rune");
    assert_eq!(result, Some(SpellResult::Success));
    let gained = mana(&f) - before;
    assert!((900..=1500).contains(&gained), "{gained}");
    assert_eq!(
        f.character
            .statistics
            .spell_statistics("Demonic Rune", 1)
            .map_or(0, |s| s.total_damage()),
        0,
        "the rune's damage hurts its user, not the target"
    );
    assert_eq!(
        use_now(&mut f, "Night Dragon's Breath").0,
        SpellStatus::OnCooldown
    );
}

/// Seal of Righteousness on every global cooldown runs the mana dry; spirit regeneration only
/// comes back once the casts stop for 5 seconds, and the rotation casts again when it can.
#[test]
fn a_fight_that_runs_dry() {
    let mut f = paladin(Race::Undead, &[]);
    let spec = "class: PALADIN\nname: Seal spam\nattack_mode: melee\ncast_if:\n  - name: Seal of Righteousness\n";
    f.ctx()
        .set_rotation(Arc::new(serde_yaml::from_str(spec).unwrap()));
    f.ctx().prepare_set_of_combat_iterations();
    let seal = highest_rank(&f, "Seal of Righteousness");
    let seal_cost = {
        let ctx = f.ctx();
        ctx.character.spells().spell(seal).resource_cost(&ctx)
    };
    pull(&mut f);
    let mut lowest = u32::MAX;
    let mut t = 0.0;
    while t < 120.0 {
        t += 0.5;
        f.advance_to(t);
        lowest = lowest.min(mana(&f));
    }
    assert!(lowest < seal_cost, "ran dry: {lowest}");
    let regenerated = f
        .character
        .statistics
        .resource_statistics(crate::character::context::REGENERATION, 1)
        .map_or(0.0, |s| s.gain(ResourceType::Mana));
    // Back to a seal's cost about every 13 ticks once dry: several casts' worth regenerated.
    assert!(regenerated >= f64::from(2 * seal_cost), "{regenerated}");
    assert!(
        mana(&f) < 2 * seal_cost,
        "it keeps spending what comes back"
    );
}
