use super::*;
use crate::spell::test_world::World;
use crate::spell::SpellResult;

const FLURRY: u32 = 12319;
const FLURRY_BUFF: u32 = 12966;
const DEEP_WOUNDS: u32 = 12834;
const DEEP_WOUNDS_BLEED: u32 = 12162;
const UNBRIDLED_WRATH: u32 = 12322;
const UNBRIDLED_WRATH_RAGE: u32 = 12964;
const BLOODTHIRST: u32 = 23881;

/// Runs `check` on the world's procs with the proc's random seeded to `seed`.
fn seeded(world: &mut World, proc: ProcId, seed: u64) {
    world.spells.procs_mut().get_mut(proc).set_seed(seed);
}

#[test]
fn procs_read_sources_rates_and_payloads_from_the_record() {
    let mut world = World::new();
    let added = world.add(DEEP_WOUNDS);
    assert!(added.spell.is_none());
    assert!(!added.enable_now, "a talent passive");
    let proc = added.proc.unwrap();
    let dw = world.spells.procs().get(proc);
    assert_eq!(dw.name(), "Deep Wounds");
    assert_eq!(dw.game_id(), DEEP_WOUNDS);
    assert_eq!(dw.rate(), ProcRate::Chance);
    // ProcTypeMask 0x11154 (swings, melee abilities, ranged, spells) with a CRITICAL hit mask.
    assert_eq!(
        dw.sources(),
        &[ProcSource::MeleeCritical, ProcSource::SpellCritical]
    );
    assert!(dw.procs_from_source(ProcSource::MeleeCritical));
    assert!(!dw.procs_from_source(ProcSource::MeleeHit));
    assert_eq!(
        dw.proc_range(ProcSource::MeleeCritical, &world),
        PROC_ROLL_RANGE
    );
    assert!(!dw.spell().is_enabled());

    let uw = world.add(UNBRIDLED_WRATH).proc.unwrap();
    let uw = world.spells.procs().get(uw);
    assert_eq!(
        uw.sources(),
        &[ProcSource::MainhandSwing, ProcSource::OffhandSwing],
        "ProcTypeMask 0x4 with the default HIT | CRITICAL mask"
    );
    assert_eq!(uw.proc_range(ProcSource::MainhandSwing, &world), 6000);
    assert_eq!(
        world.spells.proc_by_game_id(UNBRIDLED_WRATH),
        Some(ProcId(1))
    );
    assert_eq!(
        world.spells.procs().find_by_game_id(DEEP_WOUNDS),
        Some(proc)
    );
    assert_eq!(world.spells.procs().find_by_name("Deep Wounds"), Some(proc));
}

#[test]
fn ppm_procs_use_the_triggering_weapon_speed() {
    let mut world = World::new();
    let db = crate::spell::test_world::db();
    let mut record = (**db.get(UNBRIDLED_WRATH).unwrap()).clone();
    record.id = 999_001;
    record.aura_options.ppm = 2.0;
    let setup = crate::spell::SpellSetup::plain(record);
    let added = world
        .spells
        .add_spell_with(setup, db.overrides(), 0, &mut world.raid);
    let proc = added.proc.unwrap();
    let p = world.spells.procs().get(proc);
    assert_eq!(p.rate(), ProcRate::Ppm(2.0));
    // 2 ppm × 2.6 s / 60 = 8.67 %; the offhand at 1.8 s gives 6 %.
    assert_eq!(p.proc_range(ProcSource::MainhandSwing, &world), 867);
    assert_eq!(p.proc_range(ProcSource::OffhandSwing, &world), 600);
    world.oh_speed = None;
    assert_eq!(p.proc_range(ProcSource::OffhandSwing, &world), 0);
}

#[test]
#[should_panic(expected = "is not a passive spell")]
fn non_passive_spells_cannot_be_procs() {
    let mut world = World::new();
    world.add(BLOODTHIRST);
    let id = world.spell_id(BLOODTHIRST);
    let spell = world.spells.take_spell(id);
    let _ = Proc::new(spell, 1);
}

#[test]
fn conditions_follow_the_passive_aura() {
    let mut world = World::new();
    let proc = world.add(DEEP_WOUNDS).proc.unwrap();
    assert!(
        !world
            .spells
            .procs()
            .get(proc)
            .conditions_fulfilled(ProcSource::MeleeCritical, &world),
        "the aura is not up before the proc is enabled"
    );
    let mut procs = world.spells.take_procs();
    procs.enable(proc, &mut world);
    world.spells.put_procs(procs);
    assert!(world.spells.procs().is_enabled(proc));
    assert!(world
        .spells
        .procs()
        .get(proc)
        .conditions_fulfilled(ProcSource::MeleeCritical, &world));

    // Deep Wounds needs a melee weapon (SpellEquippedItems): unequipping drops the aura.
    world.weapon_ok = false;
    let mut procs = world.spells.take_procs();
    procs
        .get_mut(proc)
        .spell_mut()
        .reevaluate_passive(&mut world);
    world.spells.put_procs(procs);
    assert!(!world
        .spells
        .procs()
        .get(proc)
        .conditions_fulfilled(ProcSource::MeleeCritical, &world));
}

#[test]
fn deep_wounds_procs_on_crits_and_bleeds_for_its_aura_duration() {
    let mut world = World::new();
    world.learn(DEEP_WOUNDS_BLEED);
    let proc = world.learn(DEEP_WOUNDS).proc.unwrap();
    // Talent rank 1: 20 % of the average weapon damage over the 12 s Deep Wound aura.
    world.set_spell_effect_value(DEEP_WOUNDS, 0, 20.0);

    assert!(world.run_proc_check(ProcSource::MeleeHit).is_empty());
    let reports = world.run_proc_check(ProcSource::MeleeCritical);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].0, proc);
    assert_eq!(reports[0].1.result, SpellResult::Success);
    assert_eq!(reports[0].1.triggered.len(), 1);
    assert_eq!(reports[0].1.triggered[0].0, DEEP_WOUNDS_BLEED);
    assert_eq!(world.trigger_log, vec![(DEEP_WOUNDS_BLEED, Some(20.0))]);
    let p = world.spells.procs().get(proc);
    assert_eq!(
        (p.attempts(), p.procs()),
        (1, 1),
        "unrelated sources are not attempts"
    );
    assert_eq!(p.current_source(), Some(ProcSource::MeleeCritical));

    let bleed = world.spell(DEEP_WOUNDS_BLEED);
    assert!(bleed.is_periodic());
    assert_eq!(bleed.trigger_value(), Some(20.0));
    let marker = bleed.marker_buff().unwrap();
    assert!(world.buff(marker).is_active());
    assert_eq!(world.buff(marker).duration(), Some(12.0));

    // 200 average damage × 20 % = 40 over 4 ticks.
    world.run(12.5);
    assert_eq!(world.ticks.len(), 4);
    assert_eq!(world.ticks.iter().map(|t| t.damage).sum::<u32>(), 40);
    assert!(!world.buff(marker).is_active());
}

#[test]
fn flurry_hands_its_rank_value_to_the_haste_buff_and_loses_charges_on_swings() {
    let mut world = World::new();
    world.learn(FLURRY_BUFF);
    let proc = world.learn(FLURRY).proc.unwrap();
    world.set_spell_effect_value(FLURRY, 0, 25.0);
    let buff = world.spell(FLURRY_BUFF).marker_buff().unwrap();
    assert_eq!(world.buff(buff).base_charges(), 3);
    assert_eq!(
        world.buff(buff).charge_sources(),
        &[ProcSource::MainhandSwing, ProcSource::OffhandSwing]
    );

    let reports = world.run_proc_check(ProcSource::MeleeCritical);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].0, proc);
    assert_eq!(world.trigger_log, vec![(FLURRY_BUFF, None)]);
    assert!(world.buff(buff).is_active());
    assert_eq!(world.buff(buff).effects[0].value(), 25.0);
    assert!((world.stats.get_melee_attack_speed_mod() - 1.25).abs() < 1e-9);
    assert_eq!(world.buff(buff).charges(), 3);

    assert_eq!(
        world.spells.charge_consumers(ProcSource::MainhandSwing),
        vec![buff]
    );
    assert!(world
        .spells
        .charge_consumers(ProcSource::MeleeHit)
        .is_empty());
    world.consume_charges(ProcSource::MainhandSwing);
    world.consume_charges(ProcSource::OffhandSwing);
    assert_eq!(world.buff(buff).charges(), 1);
    assert!(world.buff(buff).is_active());
    world.consume_charges(ProcSource::MainhandSwing);
    assert!(!world.buff(buff).is_active());
    assert!((world.stats.get_melee_attack_speed_mod() - 1.0).abs() < 1e-9);
    assert!(world
        .spells
        .charge_consumers(ProcSource::MainhandSwing)
        .is_empty());
}

#[test]
fn chance_procs_roll_and_nested_sources_are_checked() {
    let mut world = World::new();
    world.learn(UNBRIDLED_WRATH_RAGE);
    let proc = world.learn(UNBRIDLED_WRATH).proc.unwrap();
    world.rage = 0;
    let mut fired = 0;
    for seed in 0..50 {
        seeded(&mut world, proc, seed);
        let reports = world.run_proc_check(ProcSource::MainhandSwing);
        fired += reports.len();
    }
    assert!(fired > 15 && fired < 45, "60 % chance: {fired} of 50");
    assert_eq!(world.rage, fired as u32);
    assert_eq!(world.spells.procs().get(proc).procs(), fired as u32);
    assert_eq!(world.spells.procs().get(proc).attempts(), 50);

    let mut procs = world.spells.take_procs();
    procs.prepare_set_of_combat_iterations();
    assert_eq!(procs.get(proc).attempts(), 0);
    procs.disable(proc, &mut world);
    assert!(!procs.is_enabled(proc));
    assert!(procs
        .run_proc_check(ProcSource::MainhandSwing, &mut world)
        .is_empty());
    procs.enable(proc, &mut world);
    procs.clear_all(&mut world);
    assert!(procs.enabled().is_empty());
    world.spells.put_procs(procs);
}

#[test]
fn internal_cooldowns_are_enforced() {
    let mut world = World::new();
    let db = crate::spell::test_world::db();
    let mut record = (**db.get(DEEP_WOUNDS).unwrap()).clone();
    record.id = 999_002;
    record.aura_options.proc_category_recovery_ms = 5000;
    let setup = crate::spell::SpellSetup::plain(record);
    let added = world
        .spells
        .add_spell_with(setup, db.overrides(), 0, &mut world.raid);
    let proc = added.proc.unwrap();
    assert!(world
        .spells
        .procs()
        .get(proc)
        .spell()
        .cooldown_id()
        .is_some());
    let mut procs = world.spells.take_procs();
    procs.enable(proc, &mut world);
    world.spells.put_procs(procs);

    assert_eq!(world.run_proc_check(ProcSource::MainhandSwing).len(), 1);
    assert!(!world.spells.procs().get(proc).is_ready(&world));
    assert!(world.run_proc_check(ProcSource::MainhandSwing).is_empty());
    world.advance_to(5.0);
    assert!(world.spells.procs().get(proc).is_ready(&world));
    assert_eq!(world.run_proc_check(ProcSource::MainhandSwing).len(), 1);

    let mut procs = world.spells.take_procs();
    procs.reset(&mut world);
    world.spells.put_procs(procs);
    assert!(world.spells.procs().get(proc).is_ready(&world));
    assert_eq!(world.spells.procs().get(proc).current_source(), None);
}

#[test]
fn procs_do_not_retrigger_themselves_within_one_check() {
    let mut world = World::new();
    world.learn(DEEP_WOUNDS_BLEED);
    let proc = world.learn(DEEP_WOUNDS).proc.unwrap();
    let mut procs = world.spells.take_procs();
    procs.ignore_in_next_check(proc);
    assert!(procs
        .run_proc_check(ProcSource::MeleeCritical, &mut world)
        .is_empty());
    assert_eq!(
        procs
            .run_proc_check(ProcSource::MeleeCritical, &mut world)
            .len(),
        1
    );
    world.spells.put_procs(procs);
}

#[test]
#[should_panic(expected = "manually triggered")]
fn manual_source_cannot_be_checked() {
    let mut world = World::new();
    world.run_proc_check(ProcSource::Manual);
}
