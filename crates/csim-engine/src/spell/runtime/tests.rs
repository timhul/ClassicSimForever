use super::*;
use crate::buff::BuffKind;
use crate::spell::dbc::SpellAttr1;
use crate::spell::test_world::{db_with, World};
use crate::spell::MAX_RANK;
use crate::stance::Stance;

const HEROIC_STRIKE: u32 = 78;
const BLOODTHIRST: u32 = 23881;
const EXECUTE: u32 = 20662;
const EXECUTE_MARKER: u32 = 26651;
const BATTLE_SHOUT_6: u32 = 11551;
const BATTLE_SHOUT_7: u32 = 25289;
const BATTLE_STANCE: u32 = 2457;
const BERSERKER_STANCE: u32 = 2458;
const BERSERKER_STANCE_PASSIVE: u32 = 7381;
const OVERPOWER: u32 = 11585;
const REVENGE: u32 = 25288;
const BLOODRAGE: u32 = 2687;
const BLOODRAGE_BUFF: u32 = 29131;
const REND: u32 = 11574;
const SUNDER_ARMOR: u32 = 11597;
const SLAM: u32 = 1464;
const IMPROVED_SLAM_RANK_2: u32 = 1310197;
const ANGER_MANAGEMENT: u32 = 12296;
const SWEEPING_STRIKES: u32 = 12292;
const IMPROVED_HEROIC_STRIKE: u32 = 12282;
const IMPROVED_OVERPOWER: u32 = 12290;
const IMPALE: u32 = 16493;
const IMPROVED_SLAM: u32 = 12862;
const TWO_HANDED_SPEC: u32 = 12163;

#[test]
fn construction_reads_the_record_and_validates_handles() {
    let mut world = World::new();
    let added = world.add(BLOODTHIRST);
    assert!(added.enable_now, "trainable spells are enabled right away");
    assert!(added.in_rank_group);
    let bt = world.spell(BLOODTHIRST);
    assert_eq!(bt.game_id(), BLOODTHIRST);
    assert_eq!(bt.name(), "Bloodthirst");
    assert_eq!(bt.rank(), 1);
    assert_eq!(bt.resource_type(), Some(ResourceType::Rage));
    assert_eq!(bt.resource_cost(&world), 30);
    assert_eq!(bt.combo_point_cost(), 0);
    assert!(bt.triggers_gcd());
    assert!(!bt.is_passive() && !bt.is_on_next_swing() && !bt.has_cast_time());
    assert!(
        bt.cooldown_id().is_none(),
        "Bloodthirst has a category cooldown only"
    );
    assert!(bt.category_cooldown_id().is_some());
    assert_eq!(bt.category_cooldown_seconds(&world), 6.0);
    assert_eq!(bt.effects().len(), 2);
    assert!(bt.marker_buff().is_none(), "the run-speed aura is pruned");
    assert!(!bt.is_periodic());
    assert_eq!(bt.threat_override().flat, 0.0);
    assert!(!bt.is_enabled());
    assert!(bt.is_rank_learned(&world));
    world.level = 39;
    assert!(!world.spell(BLOODTHIRST).is_rank_learned(&world));

    world.add(HEROIC_STRIKE);
    let hs = world.spell(HEROIC_STRIKE);
    assert!(hs.is_on_next_swing() && !hs.triggers_gcd());
    assert_eq!(hs.resource_cost(&world), 15);
    assert_eq!(hs.threat_override().flat, 145.0);
    assert!(hs.marker_buff().is_none());
}

#[test]
#[should_panic(expected = "has a cooldown but no cooldown control")]
fn spells_with_a_cooldown_need_a_handle() {
    let db = crate::spell::test_world::db();
    let setup = SpellSetup::from_db(&db, BLOODRAGE).unwrap();
    let _ = Spell::new(setup, None, None, None);
}

#[test]
#[should_panic(expected = "applies auras but no marker buff")]
fn aura_spells_need_a_marker_buff() {
    let db = crate::spell::test_world::db();
    let setup = SpellSetup::from_db(&db, BATTLE_SHOUT_6).unwrap();
    let _ = Spell::new(setup, None, None, None);
}

#[test]
fn status_checks_in_c_plus_plus_order() {
    let mut world = World::new();
    world.add(BLOODTHIRST);
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::NotEnabled);
    let id = world.spell_id(BLOODTHIRST);
    world.with_spell(id, |spell, world| spell.enable(world));
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::Available);

    world.start_global_cooldown();
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::OnGcd);
    world.next_gcd = 0.0;
    world.casting = true;
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::CastInProgress);
    world.casting = false;

    world.rolls.push_back(PhysicalAttackResult::Hit);
    world.perform(BLOODTHIRST);
    world.next_gcd = 0.0;
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::OnCooldown);
    assert_eq!(world.spell(BLOODTHIRST).cooldown_remaining(&world), 6.0);
    world.advance_to(6.0);
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::Available);

    world.rage = 29;
    assert_eq!(
        world.status(BLOODTHIRST),
        SpellStatus::InsufficientResources
    );
    world.rage = 100;
    world.start_stance_cooldown();
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::OnStanceCooldown);
    world.next_stance_cd = 0.0;

    let ignored = world.learn(SWEEPING_STRIKES);
    assert!(
        !ignored.in_rank_group,
        "IGNORED spells stay out of the rank groups"
    );
    assert_eq!(world.status(SWEEPING_STRIKES), SpellStatus::NotSupported);
}

#[test]
fn restrictions_map_to_statuses() {
    let mut world = World::new();
    world.learn(OVERPOWER);
    world.learn(EXECUTE);
    world.learn(REVENGE);

    // Overpower: 5 rage, 1 combo point (the dodge marker), Battle Stance, a melee weapon.
    assert_eq!(
        world.status(OVERPOWER),
        SpellStatus::InsufficientComboPoints
    );
    world.combo_points = 1;
    assert_eq!(world.status(OVERPOWER), SpellStatus::Available);
    world.stance = Stance::Berserker;
    assert_eq!(world.status(OVERPOWER), SpellStatus::InBerserkerStance);
    world.stance = Stance::Battle;
    world.weapon_ok = false;
    assert_eq!(world.status(OVERPOWER), SpellStatus::IncorrectWeaponType);
    world.weapon_ok = true;

    // Execute: the target below 20 % is the last 20 % of the encounter.
    assert_eq!(world.status(EXECUTE), SpellStatus::NotInExecuteRange);
    world.advance_to(240.0);
    assert_eq!(world.status(EXECUTE), SpellStatus::Available);
    world.stance = Stance::Defensive;
    assert_eq!(world.status(EXECUTE), SpellStatus::InDefensiveStance);

    // Revenge: Defensive Stance and the "after a dodge / parry / block" aura state.
    assert_eq!(world.status(REVENGE), SpellStatus::BuffInactive);
    world.caster_states.push(AuraState::Defensive);
    assert_eq!(world.status(REVENGE), SpellStatus::Available);
    world.stance = Stance::Battle;
    assert_eq!(world.status(REVENGE), SpellStatus::InBattleStance);
}

#[test]
fn perform_runs_effects_pays_cost_and_reports_damage() {
    let mut world = World::new();
    world.learn(BLOODTHIRST);

    // 30 base + 35 % of 1000 attack power.
    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.perform(BLOODTHIRST);
    assert_eq!(report.result, SpellResult::Success);
    assert_eq!(report.resource_cost, 30);
    assert_eq!(report.resource_lost, 30.0);
    assert_eq!(world.rage, 70);
    let attack = report.attack.unwrap();
    assert_eq!(attack.result, PhysicalAttackResult::Hit);
    assert_eq!(attack.damage, 380);
    assert_eq!(attack.threat, 380.0);
    assert_eq!(attack.execution_time, 1.5);
    assert_eq!(report.proc_sources, vec![ProcSource::MainhandSpell]);
    assert!(report.buff.is_none());
    assert!(world.on_global_cooldown());
    assert!(world.aura_log.is_empty());
    assert_eq!(
        world.can_crits,
        vec![true],
        "the second effect reuses the roll"
    );
    assert_eq!(world.extra_crits, vec![0]);

    world.run(10.5);
    world.rolls.push_back(PhysicalAttackResult::Critical);
    let report = world.perform(BLOODTHIRST);
    assert_eq!(report.attack.unwrap().damage, 760);
    assert_eq!(
        report.proc_sources,
        vec![ProcSource::MainhandSpell, ProcSource::MeleeCritical],
        "a crit is a landed ability and a crit"
    );

    world.armor = 3731;
    world.run(20.5);
    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.perform(BLOODTHIRST);
    let reduction = Mechanics::reduction_from_armor(3731, 60);
    assert_eq!(
        report.attack.unwrap().damage,
        (380.0 * (1.0 - reduction)).round() as u32
    );
}

#[test]
fn avoided_attacks_refund_80_percent_of_the_cost() {
    let mut world = World::new();
    world.learn(BLOODTHIRST);
    assert!(world.spell(BLOODTHIRST).record().refunds_power_on_miss());
    world.rolls.push_back(PhysicalAttackResult::Dodge);
    let report = world.perform(BLOODTHIRST);
    assert_eq!(report.result, SpellResult::Failure);
    assert_eq!(report.resource_cost, 30);
    assert_eq!(report.resource_lost, 6.0, "30 − 30 × 0.8");
    assert_eq!(world.rage, 94);
    assert_eq!(report.attack.unwrap().damage, 0);
    assert_eq!(report.proc_sources, vec![ProcSource::MeleeDodge]);
    assert!(report.buff.is_none(), "a failed cast applies no buff");
    assert_eq!(world.spell(BLOODTHIRST).last_result(), SpellResult::Failure);

    for (i, (roll, source)) in [
        (PhysicalAttackResult::Miss, ProcSource::MeleeMiss),
        (PhysicalAttackResult::Parry, ProcSource::MeleeParry),
    ]
    .into_iter()
    .enumerate()
    {
        world.advance_to(6.0 * (i + 1) as f64);
        world.rolls.push_back(roll);
        let report = world.perform(BLOODTHIRST);
        assert_eq!(report.resource_lost, 6.0);
        assert_eq!(world.rage, 94 - 6 * (i as u32 + 1));
        assert_eq!(report.proc_sources, vec![source]);
    }
}

#[test]
fn avoided_attacks_without_the_refund_flag_pay_the_full_cost() {
    let mut world = World::with_db(db_with(|file| {
        let bt = file
            .spells
            .iter_mut()
            .find(|s| s.id == BLOODTHIRST)
            .unwrap();
        bt.attributes[1] &= !SpellAttr1::DISCOUNT_POWER_ON_MISS.bits();
    }));
    world.learn(BLOODTHIRST);
    assert!(!world.spell(BLOODTHIRST).record().refunds_power_on_miss());
    world.rolls.push_back(PhysicalAttackResult::Dodge);
    let report = world.perform(BLOODTHIRST);
    assert_eq!(report.result, SpellResult::Failure);
    assert_eq!(report.resource_lost, 30.0);
    assert_eq!(world.rage, 70);
}

#[test]
#[should_panic(expected = "insufficient resource")]
fn performing_without_resources_panics() {
    let mut world = World::new();
    world.learn(BLOODTHIRST);
    world.rage = 10;
    world.perform(BLOODTHIRST);
}

#[test]
fn execute_converts_the_remaining_rage_and_triggers_its_marker() {
    let mut world = World::new();
    world.learn(EXECUTE);
    world.add(EXECUTE_MARKER);
    let marker = world.spell_id(EXECUTE_MARKER);
    world.with_spell(marker, |spell, world| spell.enable(world));

    // 600 + (100 − 15) × 1.5 × 10; all rage is consumed.
    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.perform(EXECUTE);
    assert_eq!(report.attack.unwrap().damage, 1875);
    assert_eq!(report.resource_cost, 15);
    assert_eq!(report.resource_lost, 100.0);
    assert_eq!(world.rage, 0);
    assert_eq!(report.triggered.len(), 1);
    assert_eq!(report.triggered[0].0, EXECUTE_MARKER);
    assert_eq!(report.triggered[0].1.result, SpellResult::Success);
    assert_eq!(world.trigger_log, vec![(EXECUTE_MARKER, None)]);
    assert!(world.aura_active(EXECUTE_MARKER));

    // A dodge refunds 80 % of the 15 and triggers nothing.
    world.rage = 50;
    world.advance_to(2.0);
    world.rolls.push_back(PhysicalAttackResult::Dodge);
    let report = world.perform(EXECUTE);
    assert_eq!(report.resource_lost, 3.0);
    assert_eq!(world.rage, 47);
    assert!(report.triggered.is_empty());
}

#[test]
fn buff_only_spells_apply_their_buff_with_level_scaled_values() {
    let mut world = World::new();
    world.learn(BATTLE_SHOUT_6);
    let shout = world.spell(BATTLE_SHOUT_6);
    assert_eq!(shout.effects().len(), 0);
    let marker = shout.marker_buff().unwrap();
    assert_eq!(world.buff(marker).kind(), BuffKind::PartyBuff { party: 0 });
    assert_eq!(world.buff(marker).duration(), Some(180.0));
    assert!(
        world.buff(marker).is_enabled(),
        "party buffs are enabled by the raid"
    );

    let report = world.perform(BATTLE_SHOUT_6);
    assert_eq!(report.result, SpellResult::Success);
    assert!(report.attack.is_none());
    assert_eq!(world.rage, 90);
    assert!(matches!(report.buff, Some(BuffApplication::Applied { .. })));
    assert!(world.buff(marker).is_active());
    // 111 + 0.6 × (60 − 52) = 115.8.
    assert_eq!(world.stats.base_stats().get_base_melee_ap(), 116);
    assert_eq!(world.aura_log, vec!["+ModAttackPower"]);

    world.run(180.0);
    assert!(!world.buff(marker).is_active());
    assert_eq!(world.stats.base_stats().get_base_melee_ap(), 0);
    assert_eq!(world.aura_log, vec!["+ModAttackPower", "-ModAttackPower"]);
}

#[test]
fn rank_groups_follow_the_learn_level() {
    let mut world = World::new();
    world.learn(BATTLE_SHOUT_6);
    world.learn(BATTLE_SHOUT_7);
    let group = world.spells.rank_group("Battle Shout").unwrap();
    assert_eq!(group.rank_numbers().collect::<Vec<_>>(), vec![6, 7]);
    let learned = |id: SpellId| world.spells.spell(id).is_rank_learned(&world);
    assert_eq!(
        group.get_spell_rank(MAX_RANK, learned),
        Some(world.spell_id(BATTLE_SHOUT_7))
    );
    world.level = 55;
    let learned = |id: SpellId| world.spells.spell(id).is_rank_learned(&world);
    assert_eq!(
        group.get_spell_rank(MAX_RANK, learned),
        Some(world.spell_id(BATTLE_SHOUT_6))
    );
}

#[test]
fn stance_spells_swap_the_stance_and_share_the_stance_category() {
    let mut world = World::new();
    world.learn(BATTLE_STANCE);
    world.learn(BERSERKER_STANCE);
    world.learn(BLOODTHIRST);
    let berserker = world.spell(BERSERKER_STANCE);
    assert!(berserker.is_stance_spell());
    assert_eq!(berserker.stance(), Some(Stance::Berserker));
    assert_eq!(berserker.stance_passive(), Some(BERSERKER_STANCE_PASSIVE));
    assert!(!berserker.triggers_gcd());
    assert_eq!(
        berserker.category_cooldown_id(),
        world.spell(BATTLE_STANCE).category_cooldown_id(),
        "both stances share SpellCategory 47"
    );

    let report = world.perform(BERSERKER_STANCE);
    assert_eq!(report.result, SpellResult::Success);
    assert_eq!(world.stance, Stance::Berserker);
    assert!(world.on_stance_cooldown());
    assert!(!world.on_global_cooldown());
    assert_eq!(world.status(BATTLE_STANCE), SpellStatus::OnCooldown);
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::OnStanceCooldown);
    world.advance_to(1.0);
    assert_eq!(world.status(BATTLE_STANCE), SpellStatus::Available);
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::Available);
}

#[test]
#[should_panic(expected = "already on stance cooldown")]
fn stance_swap_during_stance_cooldown_panics() {
    let mut world = World::new();
    world.learn(BERSERKER_STANCE);
    world.start_stance_cooldown();
    world.perform(BERSERKER_STANCE);
}

#[test]
fn passives_apply_their_aura_on_enable_and_remove_it_on_disable() {
    let mut world = World::new();
    let added = world.learn(BERSERKER_STANCE_PASSIVE);
    assert!(added.enable_now && added.proc.is_none());
    assert!(!added.in_rank_group, "passives are not cast");
    let passive = world.spell(BERSERKER_STANCE_PASSIVE);
    assert!(passive.is_passive() && passive.is_enabled());
    let marker = passive.marker_buff().unwrap();
    assert!(world.buff(marker).is_active());
    assert!(world.buff(marker).is_permanent());
    assert_eq!(
        world.aura_log,
        vec![
            "+ModCritPct",
            "+ModDamagePercentTaken",
            "+ModThreat",
            "+ModAttackPowerPct"
        ]
    );
    assert!((world.stats.get_total_threat_mod() - 0.8).abs() < 1e-9);
    assert_eq!(world.stats.aura_effects().get_melee_crit_chance(), 300);

    let id = world.spell_id(BERSERKER_STANCE_PASSIVE);
    world.with_spell(id, |spell, world| spell.disable(world));
    assert!(!world.buff(marker).is_active());
    assert!(!world.buff(marker).is_enabled());
    assert!((world.stats.get_total_threat_mod() - 1.0).abs() < 1e-9);
}

#[test]
fn passives_with_equipment_conditions_are_reevaluated() {
    let mut world = World::new();
    world.weapon_ok = false;
    world.learn(TWO_HANDED_SPEC);
    let id = world.spell_id(TWO_HANDED_SPEC);
    let marker = world.spell(TWO_HANDED_SPEC).marker_buff().unwrap();
    assert!(!world.spell(TWO_HANDED_SPEC).conditions_hold(&world));
    assert!(!world.buff(marker).is_active());

    world.weapon_ok = true;
    world.with_spell(id, |spell, world| spell.reevaluate_passive(world));
    assert!(world.buff(marker).is_active());
    assert_eq!(world.aura_log, vec!["+ModDamagePercentDone"]);

    world.weapon_ok = false;
    world.with_spell(id, |spell, world| spell.reevaluate_passive(world));
    assert!(!world.buff(marker).is_active());
    assert_eq!(
        world.aura_log,
        vec!["+ModDamagePercentDone", "-ModDamagePercentDone"]
    );
}

#[test]
fn modifiers_change_cost_crit_and_crit_damage() {
    let mut world = World::new();
    world.learn(HEROIC_STRIKE);
    world.learn(OVERPOWER);
    world.learn(BLOODTHIRST);
    assert_eq!(world.spell(HEROIC_STRIKE).resource_cost(&world), 15);

    // Improved Heroic Strike: −10 stored rage = −1 rage on Heroic Strike only.
    world.learn(IMPROVED_HEROIC_STRIKE);
    assert_eq!(world.modifiers.len(), 1);
    assert_eq!(world.spell(HEROIC_STRIKE).resource_cost(&world), 14);
    assert_eq!(world.spell(BLOODTHIRST).resource_cost(&world), 30);

    // Improved Overpower: +25 % crit on Overpower, handed to the roll.
    world.learn(IMPROVED_OVERPOWER);
    assert_eq!(world.spell(OVERPOWER).crit_chance_bonus(&world), 2500);
    assert_eq!(world.spell(BLOODTHIRST).crit_chance_bonus(&world), 0);
    world.combo_points = 1;
    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.perform(OVERPOWER);
    assert_eq!(world.extra_crits, vec![2500]);
    assert_eq!(report.attack.unwrap().damage, 335, "normalized 300 + 35");
    assert_eq!(world.combo_points, 0, "combo points are spent");
    assert!(
        !world.spell(OVERPOWER).effects()[0]
            .included_outcomes()
            .dodge
    );

    // Impale: the crit bonus of Bloodthirst grows by 10 %.
    world.learn(IMPALE);
    assert!((world.spell(BLOODTHIRST).crit_damage_mod(&world) - 2.1).abs() < 1e-9);
    world.next_gcd = 0.0;
    world.rolls.push_back(PhysicalAttackResult::Critical);
    let report = world.perform(BLOODTHIRST);
    assert_eq!(report.attack.unwrap().damage, 798);

    // Disabling the talent removes its modifiers.
    let id = world.spell_id(IMPALE);
    world.with_spell(id, |spell, world| spell.disable(world));
    assert_eq!(world.spell(BLOODTHIRST).crit_damage_mod(&world), 2.0);
}

#[test]
fn effect_values_can_be_replaced_and_reapply_an_active_buff() {
    let mut world = World::new();
    world.learn(BATTLE_SHOUT_7);
    let id = world.spell_id(BATTLE_SHOUT_7);
    world.perform(BATTLE_SHOUT_7);
    assert_eq!(world.stats.base_stats().get_base_melee_ap(), 139);

    world.set_spell_effect_value(BATTLE_SHOUT_7, 0, 250.0);
    assert_eq!(world.stats.base_stats().get_base_melee_ap(), 250);
    assert_eq!(
        world.aura_log,
        vec!["+ModAttackPower", "-ModAttackPower", "+ModAttackPower"]
    );

    world.with_spell(id, |spell, world| spell.reset_effect_values(world));
    assert_eq!(world.stats.base_stats().get_base_melee_ap(), 139);

    world.learn(BLOODTHIRST);
    world.set_spell_effect_value(BLOODTHIRST, 1, 50.0);
    assert_eq!(world.spell(BLOODTHIRST).effects()[1].value(), 50.0);
    world.advance_to(2.0);
    world.rolls.push_back(PhysicalAttackResult::Hit);
    assert_eq!(world.perform(BLOODTHIRST).attack.unwrap().damage, 530);
}

#[test]
fn periodic_damage_ticks_for_the_duration() {
    let mut world = World::new();
    world.learn(REND);
    let rend = world.spell(REND);
    assert!(rend.is_periodic());
    assert_eq!(rend.periodic().unwrap().tick_rate(), 3.0);
    let marker = rend.marker_buff().unwrap();
    assert_eq!(world.buff(marker).kind(), BuffKind::UniqueDebuff);
    assert_eq!(
        rend.periodic_kind(&world),
        Some(PeriodicKind::Damage {
            per_tick: 21.0,
            ticks: 7
        })
    );

    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.perform(REND);
    assert_eq!(report.result, SpellResult::Success);
    assert!(report.attack.is_none(), "the damage comes from the ticks");
    assert_eq!(report.proc_sources, vec![ProcSource::MainhandSpell]);
    assert_eq!(world.can_crits, vec![false], "a bleed cannot crit");
    assert_eq!(world.rage, 90);
    world.run(21.5);
    assert_eq!(world.ticks.len(), 7);
    assert_eq!(world.ticks.iter().map(|t| t.damage).sum::<u32>(), 147);
    assert!((world.ticks[0].resource_cost - 10.0 / 7.0).abs() < 1e-9);
    assert!(!world.buff(marker).is_active());

    // A refresh re-arms the ticks without a second chain.
    world.next_gcd = 0.0;
    world.ticks.clear();
    world.rolls.push_back(PhysicalAttackResult::Hit);
    world.perform(REND);
    world.run(30.0);
    assert_eq!(world.ticks.len(), 2);
    world.next_gcd = 0.0;
    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.perform(REND);
    assert!(matches!(
        report.buff,
        Some(BuffApplication::Refreshed { .. })
    ));
    world.run(60.0);
    assert_eq!(
        world.ticks.len(),
        2 + 7,
        "the refresh re-arms the full tick count"
    );
}

#[test]
fn avoided_rend_applies_no_bleed_and_refunds_the_cost() {
    let mut world = World::new();
    world.learn(REND);
    let marker = world.spell(REND).marker_buff().unwrap();
    for (i, (roll, source)) in [
        (PhysicalAttackResult::Miss, ProcSource::MeleeMiss),
        (PhysicalAttackResult::Dodge, ProcSource::MeleeDodge),
        (PhysicalAttackResult::Parry, ProcSource::MeleeParry),
    ]
    .into_iter()
    .enumerate()
    {
        world.next_gcd = 0.0;
        world.rolls.push_back(roll);
        let report = world.perform(REND);
        assert_eq!(report.result, SpellResult::Failure);
        assert_eq!(report.attack.as_ref().unwrap().result, roll);
        assert_eq!(report.proc_sources, vec![source]);
        assert_eq!(report.resource_lost, 2.0, "80 % of the 10 rage is refunded");
        assert_eq!(world.rage, 100 - 2 * (i as u32 + 1));
        assert!(report.buff.is_none());
        assert!(!world.buff(marker).is_active());
    }
    world.run(30.0);
    assert!(world.ticks.is_empty());
}

#[test]
fn periodic_resource_gain_ticks_until_the_buff_expires() {
    let mut world = World::new();
    world.rage = 0;
    world.learn(BLOODRAGE);
    world.learn(BLOODRAGE_BUFF);
    let bloodrage = world.spell(BLOODRAGE);
    assert_eq!(
        bloodrage.resource_type(),
        None,
        "health costs are not modelled"
    );
    assert!(bloodrage.cooldown_id().is_some());
    assert_eq!(bloodrage.cooldown_seconds(&world), 60.0);
    assert!(!bloodrage.triggers_gcd());

    let report = world.perform(BLOODRAGE);
    assert_eq!(report.resource_gained, vec![(ResourceType::Rage, 10)]);
    assert_eq!(world.rage, 10);
    assert_eq!(report.triggered.len(), 1);
    assert_eq!(world.status(BLOODRAGE), SpellStatus::OnCooldown);
    assert_eq!(
        world.spell(BLOODRAGE_BUFF).periodic_kind(&world),
        Some(PeriodicKind::ResourceGain {
            resource: ResourceType::Rage,
            amount: 1
        })
    );

    world.run(10.5);
    assert_eq!(world.ticks.len(), 10);
    assert_eq!(world.rage, 20);
    world.run(15.0);
    assert_eq!(world.ticks.len(), 10, "no ticks after the buff expired");
}

#[test]
fn start_of_combat_passives_tick_forever() {
    let mut world = World::new();
    world.rage = 0;
    let added = world.learn(ANGER_MANAGEMENT);
    assert!(!added.enable_now, "talent passives wait for their talent");
    assert_eq!(
        world.spells.start_of_combat_spells(),
        &[added.spell.unwrap()]
    );
    let anger = world.spell(ANGER_MANAGEMENT);
    assert!(anger.is_passive() && anger.is_periodic());
    assert_eq!(anger.periodic().unwrap().tick_rate(), 3.0);
    assert!(world.buff(anger.marker_buff().unwrap()).is_active());
    world.run(9.5);
    assert_eq!(world.rage, 3);
}

#[test]
fn cast_time_spells_complete_after_the_cast_time() {
    let mut world = World::new();
    world.learn(SLAM);
    let slam = world.spell(SLAM);
    assert!(slam.has_cast_time());
    assert_eq!(slam.cast_time(&world), 1.5);
    assert_eq!(slam.category_cooldown_seconds(&world), 15.0);

    let report = world.perform(SLAM);
    assert!(report.cast_started);
    assert!(report.attack.is_none());
    assert_eq!(world.rage, 100, "the cost is paid when the cast completes");
    assert!(world.spell(SLAM).is_casting());
    assert!(world.cast_in_progress());
    assert_eq!(world.attack_log, vec!["stop"]);
    world.next_gcd = 0.0;
    assert_eq!(world.status(SLAM), SpellStatus::CastInProgress);

    world.rolls.push_back(PhysicalAttackResult::Hit);
    world.run(1.5);
    assert_eq!(world.completed_casts.len(), 1);
    let done = &world.completed_casts[0];
    assert_eq!(done.attack.unwrap().damage, 432, "400 weapon + 32");
    assert_eq!(done.attack.unwrap().execution_time, 1.5);
    assert_eq!(world.rage, 85);
    assert!(!world.cast_in_progress());
    assert_eq!(world.attack_log, vec!["stop", "reset", "start"]);
    assert_eq!(world.status(SLAM), SpellStatus::OnCooldown);

    // Haste and modifiers shorten the cast.
    world.casting_speed_mod = 1.5;
    assert!((world.spell(SLAM).cast_time(&world) - 1.0).abs() < 1e-9);
}

#[test]
fn improved_slam_shortens_the_cast_and_replaces_the_rank() {
    let mut world = World::new();
    world.learn(SLAM);
    let replacement = world.add(IMPROVED_SLAM_RANK_2);
    assert!(
        !replacement.in_rank_group,
        "hidden replacement ranks stay out"
    );
    world.with_spell(replacement.spell.unwrap(), |spell, world| {
        spell.enable(world)
    });
    let group = |world: &World| {
        world
            .spells
            .rank_group("Slam")
            .unwrap()
            .get_spell_rank(2, |_| true)
    };
    assert_eq!(group(&world), Some(world.spell_id(SLAM)));

    world.learn(IMPROVED_SLAM);
    assert_eq!(
        world.actionbar_log,
        vec![(SLAM, IMPROVED_SLAM_RANK_2, true)]
    );
    assert_eq!(group(&world), Some(world.spell_id(IMPROVED_SLAM_RANK_2)));
    assert_eq!(
        world.spells.actionbar_overrides(),
        &[(SLAM, IMPROVED_SLAM_RANK_2)]
    );
    let improved = world.spell(IMPROVED_SLAM_RANK_2);
    assert!((improved.cast_time(&world) - 1.0).abs() < 1e-9);
    assert!((improved.global_cooldown(&world) - 1.0).abs() < 1e-9);
    assert!(!improved.has_sim_flag(SimFlag::ResetsSwingTimers));

    let id = world.spell_id(IMPROVED_SLAM);
    world.with_spell(id, |spell, world| spell.disable(world));
    assert_eq!(group(&world), Some(world.spell_id(SLAM)));
    assert!(world.spells.actionbar_overrides().is_empty());
}

#[test]
fn reset_forgets_a_cast_in_progress() {
    let mut world = World::new();
    world.learn(SLAM);
    world.perform(SLAM);
    let id = world.spell_id(SLAM);
    world.with_spell(id, |spell, world| spell.reset(world));
    world.spells.reset_state();
    assert!(!world.spell(SLAM).is_casting());
    world.rolls.push_back(PhysicalAttackResult::Hit);
    world.run(1.5);
    assert!(
        world.completed_casts.is_empty(),
        "the stale cast id is ignored"
    );
    assert_eq!(world.status(SLAM), SpellStatus::Available);
}

#[test]
fn on_next_swing_spells_queue_and_fire_on_the_swing() {
    let mut world = World::new();
    world.learn(HEROIC_STRIKE);
    let id = world.spell_id(HEROIC_STRIKE);
    assert!(!world.spell(HEROIC_STRIKE).is_queued(&world));

    let report = world.perform(HEROIC_STRIKE);
    assert!(report.queued);
    assert_eq!(world.rage, 100);
    assert!(!world.on_global_cooldown());
    assert!(world.spell(HEROIC_STRIKE).is_queued(&world));
    assert_eq!(world.spells.queued_next_swing(), Some(id));
    assert_eq!(world.attack_log, vec!["queue"]);

    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.with_spell(id, |spell, world| spell.perform_on_swing(world));
    assert!(!world.spell(HEROIC_STRIKE).is_queued(&world));
    let attack = report.attack.unwrap();
    assert_eq!(attack.damage, 411, "400 weapon + 11");
    assert_eq!(attack.threat, 411.0 + 145.0);
    assert_eq!(attack.execution_time, 0.0);
    assert_eq!(world.rage, 85);
    assert_eq!(world.attack_log, vec!["queue", "unqueue"]);

    world.perform(HEROIC_STRIKE);
    world.with_spell(id, |spell, world| spell.cancel(world));
    assert_eq!(world.spells.queued_next_swing(), None);
}

#[test]
fn threat_effects_and_debuff_stacks() {
    let mut world = World::new();
    world.learn(SUNDER_ARMOR);
    let sunder = world.spell(SUNDER_ARMOR);
    assert_eq!(
        sunder.effects().len(),
        2,
        "the rolled armor debuff, then the THREAT effect"
    );
    assert!(sunder.effects()[0].is_melee_debuff());
    assert_eq!(sunder.effects()[1].kind(), SpellEffectName::Threat);
    let marker = sunder.marker_buff().unwrap();
    assert_eq!(world.buff(marker).kind(), BuffKind::SharedDebuff);
    assert_eq!(world.buff(marker).max_stacks(), 5);
    assert_eq!(world.buff(marker).priority(), crate::target::Priority::High);
    let base_armor = world.target.armor();

    world.rolls.push_back(PhysicalAttackResult::Hit);
    let report = world.perform(SUNDER_ARMOR);
    assert_eq!(report.result, SpellResult::Success);
    assert_eq!(world.can_crits, vec![false], "an aura roll cannot crit");
    let attack = report.attack.unwrap();
    assert_eq!(attack.result, PhysicalAttackResult::Hit);
    assert_eq!(attack.damage, 0);
    assert_eq!(attack.threat, 1013.0);
    assert_eq!(report.proc_sources, vec![ProcSource::MainhandSpell]);
    assert_eq!(world.target.armor(), base_armor - 450);

    for i in 1..6 {
        world.advance_to(2.0 * f64::from(i));
        world.rolls.push_back(PhysicalAttackResult::Hit);
        world.perform(SUNDER_ARMOR);
    }
    assert_eq!(world.buff(marker).stacks(), 5);
    assert_eq!(world.target.armor(), base_armor - 2250);
}

#[test]
fn avoided_sunder_armor_applies_no_stack_nor_threat() {
    let mut world = World::new();
    world.learn(SUNDER_ARMOR);
    let marker = world.spell(SUNDER_ARMOR).marker_buff().unwrap();
    let base_armor = world.target.armor();
    for (i, (roll, source)) in [
        (PhysicalAttackResult::Miss, ProcSource::MeleeMiss),
        (PhysicalAttackResult::Dodge, ProcSource::MeleeDodge),
        (PhysicalAttackResult::Parry, ProcSource::MeleeParry),
    ]
    .into_iter()
    .enumerate()
    {
        world.advance_to(2.0 * i as f64);
        world.rolls.push_back(roll);
        let report = world.perform(SUNDER_ARMOR);
        assert_eq!(report.result, SpellResult::Failure);
        let attack = report.attack.unwrap();
        assert_eq!(attack.result, roll);
        assert_eq!(attack.threat, 0.0);
        assert_eq!(report.proc_sources, vec![source]);
        assert_eq!(report.resource_lost, 3.0, "80 % of the 15 rage is refunded");
        assert!(report.buff.is_none());
        assert!(!world.buff(marker).is_active());
        assert_eq!(world.target.armor(), base_armor);
    }
}

#[test]
fn reset_clears_cooldowns_and_state() {
    let mut world = World::new();
    world.learn(BLOODTHIRST);
    world.rolls.push_back(PhysicalAttackResult::Hit);
    world.perform(BLOODTHIRST);
    world.next_gcd = 0.0;
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::OnCooldown);
    let id = world.spell_id(BLOODTHIRST);
    world.with_spell(id, |spell, world| spell.reset(world));
    assert_eq!(world.status(BLOODTHIRST), SpellStatus::Available);
    assert_eq!(
        world.spell(BLOODTHIRST).last_result(),
        SpellResult::Undetermined
    );
    assert_eq!(world.spell(BLOODTHIRST).cooldown_remaining(&world), 0.0);
}

#[test]
fn casting_time_spell_coefficient() {
    assert!((spell_coefficient_from_casting_time(1000, 60) - 1500.0 / 3500.0).abs() < 1e-9);
    assert_eq!(spell_coefficient_from_casting_time(4000, 60), 1.0);
    assert!((spell_coefficient_from_casting_time(2000, 60) - 2000.0 / 3500.0).abs() < 1e-9);
    assert!(
        (spell_coefficient_from_casting_time(2000, 10) - (2000.0 / 3500.0 - 0.375)).abs() < 1e-9
    );
    assert_eq!(spell_coefficient_from_casting_time(1500, 1), 0.0);
}

#[test]
fn cast_reports_aggregate_triggered_spells() {
    let inner = CastReport {
        proc_sources: vec![ProcSource::MainhandSpell],
        attack: Some(AttackOutcome {
            result: PhysicalAttackResult::Hit,
            damage: 40,
            threat: 40.0,
            execution_time: 0.0,
        }),
        ..CastReport::default()
    };
    let outer = CastReport {
        proc_sources: vec![ProcSource::MeleeCritical],
        attack: Some(AttackOutcome {
            result: PhysicalAttackResult::Critical,
            damage: 100,
            threat: 100.0,
            execution_time: 1.5,
        }),
        triggered: vec![(12162, inner)],
        ..CastReport::default()
    };
    assert_eq!(
        outer.all_proc_sources(),
        vec![ProcSource::MeleeCritical, ProcSource::MainhandSpell]
    );
    assert_eq!(outer.total_damage(), 140);
}
