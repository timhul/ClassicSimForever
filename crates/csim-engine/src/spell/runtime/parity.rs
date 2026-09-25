//! Parity with the shipped data: the worked examples of `data/SPELL_INSTRUCTIONS.md` §1.8 run
//! through the runtime on `data/spells/*.yaml` (build 1.60.1.69893). Where the C++
//! `TestSpellWarrior` numbers (Classic) differ from the Forever tables, the Forever value is
//! asserted and the Classic one noted.

use std::path::Path;

use super::*;
use crate::buff::BuffKind;
use crate::spell::test_world::World;
use crate::stance::Stance;

const MORTAL_STRIKE: u32 = 12294;
const BLOODTHIRST: u32 = 23881;
const SHIELD_SLAM: u32 = 23922;
const IMPALE: u32 = 16493;
const REND_7: u32 = 11574;
const SUNDER_ARMOR_5: u32 = 11597;
const FLURRY: u32 = 12319;
const FLURRY_BUFF: u32 = 12966;
const DEEP_WOUNDS: u32 = 12834;
const DEEP_WOUNDS_BLEED: u32 = 12162;
const HEROIC_STRIKE_9: u32 = 25286;
const IMPROVED_HEROIC_STRIKE: u32 = 12282;
const BERSERKER_STANCE: u32 = 2458;
const BERSERKER_STANCE_PASSIVE: u32 = 7381;
const EXECUTE_5: u32 = 20662;
const EXECUTE_MARKER: u32 = 26651;
const REVENGE_6: u32 = 25288;
const WHIRLWIND: u32 = 1680;
const BLOODRAGE: u32 = 2687;
const BLOODRAGE_BUFF: u32 = 29131;
const BATTLE_SHOUT_7: u32 = 25289;
const OVERPOWER_4: u32 = 11585;

fn shipped() -> World {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/spells");
    World::with_db(SpellDb::load(&dir).expect("shipped spell data loads"))
}

#[test]
fn mortal_strike_and_the_spells_sharing_its_category() {
    let mut world = shipped();
    world.learn(MORTAL_STRIKE);
    world.learn(BLOODTHIRST);
    world.learn(SHIELD_SLAM);
    let ms = world.spell(MORTAL_STRIKE);
    assert_eq!(ms.rank(), 1);
    assert_eq!(ms.resource_cost(&world), 30);
    assert_eq!(ms.category_cooldown_seconds(), 6.0);
    assert!(ms.triggers_gcd());
    assert!(ms.marker_buff().is_none(), "the healing debuff is pruned");
    assert_eq!(
        ms.category_cooldown_id(),
        world.spell(BLOODTHIRST).category_cooldown_id(),
        "category 971"
    );
    assert_eq!(
        ms.category_cooldown_id(),
        world.spell(SHIELD_SLAM).category_cooldown_id()
    );

    // Normalized weapon damage + 85 (the Classic value too).
    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.perform(MORTAL_STRIKE);
    assert_eq!(report.attack.unwrap().damage, 385);
    assert_eq!(world.rage, 70);
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::OnGcd);
    world.next_gcd = 0.0;
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::OnCooldown);
    assert_eq!(world.status(SHIELD_SLAM), SpellStatus::OnCooldown);

    // Impale's mask word 0 covers Mortal Strike, Bloodthirst and Shield Slam alike.
    world.learn(IMPALE);
    for id in [MORTAL_STRIKE, BLOODTHIRST, SHIELD_SLAM] {
        assert!(
            (world.spell(id).crit_damage_mod(&world) - 2.1).abs() < 1e-9,
            "{id}"
        );
    }
}

#[test]
fn rend_rank_7_deals_147_over_21_seconds() {
    let mut world = shipped();
    world.learn(REND_7);
    assert_eq!(world.spell(REND_7).resource_cost(&world), 10);
    world.perform(REND_7);
    world.run(21.5);
    assert_eq!(world.ticks.len(), 7);
    assert_eq!(world.ticks.iter().map(|t| t.damage).sum::<u32>(), 147);
}

#[test]
fn sunder_armor_rank_5_stacks_five_times_with_1013_threat() {
    let mut world = shipped();
    world.learn(SUNDER_ARMOR_5);
    let marker = world.spell(SUNDER_ARMOR_5).marker_buff().unwrap();
    assert_eq!(world.buff(marker).kind(), BuffKind::SharedDebuff);
    assert_eq!(world.buff(marker).duration(), Some(30.0));
    let base_armor = world.target.armor();
    for i in 0..6 {
        world.advance_to(2.0 * f64::from(i));
        let report = world.perform(SUNDER_ARMOR_5);
        assert_eq!(report.attack.unwrap().threat, 1013.0);
        assert_eq!(report.resource_lost, 15);
    }
    assert_eq!(world.buff(marker).stacks(), 5);
    assert_eq!(world.target.armor(), base_armor - 5 * 450);
}

#[test]
fn flurry_gives_three_charges_of_haste_on_crits() {
    let mut world = shipped();
    world.learn(FLURRY_BUFF);
    world.learn(FLURRY);
    // The talent's rank value replaces the DUMMY's base points (the curve gives 30 at rank 5).
    world.set_spell_effect_value(FLURRY, 0, 30.0);
    let buff = world.spell(FLURRY_BUFF).marker_buff().unwrap();
    assert_eq!(world.buff(buff).base_charges(), 3);
    assert_eq!(world.buff(buff).duration(), Some(15.0));

    assert!(
        world.run_proc_check(ProcSource::MainhandSwing).is_empty(),
        "a plain landed swing does not proc Flurry"
    );
    assert_eq!(world.run_proc_check(ProcSource::MeleeCritical).len(), 1);
    assert!((world.stats.get_melee_attack_speed_mod() - 1.3).abs() < 1e-9);
    for _ in 0..3 {
        assert!(world.buff(buff).is_active());
        world.consume_charges(ProcSource::MainhandSwing);
    }
    assert!(!world.buff(buff).is_active());
    assert!((world.stats.get_melee_attack_speed_mod() - 1.0).abs() < 1e-9);
}

#[test]
fn deep_wounds_bleeds_for_twelve_seconds_after_a_crit() {
    let mut world = shipped();
    world.learn(DEEP_WOUNDS_BLEED);
    world.learn(DEEP_WOUNDS);
    world.set_spell_effect_value(DEEP_WOUNDS, 0, 60.0); // rank 3
    assert_eq!(world.run_proc_check(ProcSource::MeleeCritical).len(), 1);
    assert_eq!(world.trigger_log, vec![(DEEP_WOUNDS_BLEED, Some(60.0))]);
    world.run(12.5);
    assert_eq!(world.ticks.len(), 4);
    // 60 % of the 200 average weapon damage: 120 over four ticks.
    assert_eq!(world.ticks.iter().map(|t| t.damage).sum::<u32>(), 120);
}

#[test]
fn improved_heroic_strike_takes_one_rage_per_rank() {
    let mut world = shipped();
    world.learn(HEROIC_STRIKE_9);
    let hs = world.spell(HEROIC_STRIKE_9);
    assert_eq!(hs.resource_cost(&world), 15);
    assert!(hs.is_on_next_swing());
    world.learn(IMPROVED_HEROIC_STRIKE);
    assert_eq!(world.spell(HEROIC_STRIKE_9).resource_cost(&world), 14);
    world.set_spell_effect_value(IMPROVED_HEROIC_STRIKE, 0, -30.0); // rank 3
    assert_eq!(world.spell(HEROIC_STRIKE_9).resource_cost(&world), 12);

    // Rank 9 adds 157 weapon damage in Forever (138 in Classic) with 145 innate threat.
    let id = world.spell_id(HEROIC_STRIKE_9);
    world.perform(HEROIC_STRIKE_9);
    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.with_spell(id, |spell, world| spell.perform_on_swing(world));
    let attack = report.attack.unwrap();
    assert_eq!(attack.damage, 400 + 157);
    assert_eq!(attack.threat, 557.0 + 145.0);
    assert_eq!(world.rage, 88);
}

#[test]
fn berserker_stance_and_its_passive() {
    let mut world = shipped();
    world.learn(BERSERKER_STANCE);
    assert_eq!(
        world.spell(BERSERKER_STANCE).stance_passive(),
        Some(BERSERKER_STANCE_PASSIVE)
    );
    world.perform(BERSERKER_STANCE);
    assert_eq!(world.stance, Stance::Berserker);
    assert_eq!(world.aura_log, vec!["+ModShapeshift"]);
    world.aura_log.clear();

    world.learn(BERSERKER_STANCE_PASSIVE);
    assert_eq!(
        world.aura_log,
        vec![
            "+ModCritPct",
            "+ModDamagePercentTaken",
            "+ModThreat",
            "+ModAttackPowerPct"
        ]
    );
    assert_eq!(world.stats.aura_effects().get_melee_crit_chance(), 300);
    assert!((world.stats.get_total_threat_mod() - 0.8).abs() < 1e-9);
}

#[test]
fn execute_rank_5_converts_rage_at_15_per_point() {
    // 600 + 15 per rage above the cost, as in Classic (C++ TestSpellWarrior).
    let mut world = shipped();
    world.learn(EXECUTE_5);
    world.learn(EXECUTE_MARKER);
    world.advance_to(250.0);
    assert_eq!(world.status(EXECUTE_5), SpellStatus::Available);
    world.rage = 50;
    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.perform(EXECUTE_5);
    assert_eq!(report.attack.unwrap().damage, 600 + 35 * 15);
    assert_eq!(world.rage, 0);
}

#[test]
fn revenge_rank_6_and_whirlwind_and_overpower_use_their_table_values() {
    let mut world = shipped();
    world.learn(REVENGE_6);
    world.learn(WHIRLWIND);
    world.learn(OVERPOWER_4);
    // Revenge r6: 153 ± 10 % in Forever (64–78 in Classic); the mock RNG returns the midpoint.
    world.stance = Stance::Defensive;
    world.caster_states.push(AuraState::Defensive);
    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.perform(REVENGE_6);
    assert_eq!(report.attack.unwrap().damage, 153);
    assert_eq!(report.attack.unwrap().threat, 153.0 + 355.0);
    assert_eq!(world.rage, 95);

    // Whirlwind: plain normalized weapon damage, 25 rage, category 891 for 10 s.
    world.stance = Stance::Berserker;
    world.next_gcd = 0.0;
    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.perform(WHIRLWIND);
    assert_eq!(report.attack.unwrap().damage, 300);
    assert_eq!(world.rage, 70);
    assert_eq!(world.spell(WHIRLWIND).category_cooldown_seconds(), 10.0);

    // Overpower r4: normalized + 35 (Classic too), cannot be dodged / parried / blocked.
    world.stance = Stance::Battle;
    world.combo_points = 1;
    world.next_gcd = 0.0;
    world.rolls.push_back(PhysicalAttackResult::Critical);
    let report = world.perform(OVERPOWER_4);
    assert_eq!(report.attack.unwrap().damage, 335 * 2);
    assert_eq!(world.rage, 65);
    assert_eq!(world.combo_points, 0);
    let included = world.spell(OVERPOWER_4).effects()[0].included_outcomes();
    assert!(!included.dodge && !included.parry && !included.block && included.miss);
}

#[test]
fn bloodrage_and_battle_shout() {
    let mut world = shipped();
    world.rage = 0;
    world.learn(BLOODRAGE);
    world.learn(BLOODRAGE_BUFF);
    world.learn(BATTLE_SHOUT_7);
    world.perform(BLOODRAGE);
    assert_eq!(world.rage, 10);
    world.run(10.5);
    assert_eq!(world.rage, 20, "1 rage per second for 10 s");
    assert_eq!(world.spell(BLOODRAGE).cooldown_seconds(&world), 60.0);

    // Battle Shout r7 gives 139 attack power in Forever (232 in Classic) for 3 minutes.
    let report = world.perform(BATTLE_SHOUT_7);
    assert!(matches!(report.buff, Some(BuffApplication::Applied { .. })));
    assert_eq!(world.stats.base_stats().get_base_melee_ap(), 139);
    assert_eq!(world.rage, 10);
    let marker = world.spell(BATTLE_SHOUT_7).marker_buff().unwrap();
    assert_eq!(world.buff(marker).duration(), Some(180.0));
    assert_eq!(world.buff(marker).kind(), BuffKind::PartyBuff { party: 0 });
}
