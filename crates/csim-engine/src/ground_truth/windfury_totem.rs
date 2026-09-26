//! `Windfury-Totem.md`: a main-hand temporary enchant with a 20 % chance on hit to grant one
//! extra attack and a two-charge attack power aura.
//!
//! The records below are the Forever client tables' Windfury Totem Rank 3: the party passive
//! 10612 (`ProcChance` 20, `ProcTypeMask` 0x14 = melee swings and melee abilities,
//! `ProcCategoryRecovery` 100 ms) and its payload 10610 (`MOD_ATTACK_POWER` 246, one
//! `ADD_EXTRA_ATTACKS`, 1000 ms, two charges used by melee swings). The wiki documents the
//! Classic numbers — 315 attack power, a 1.5 s aura and a 1.5 s internal cooldown; the tests
//! follow the tables for those three values and the wiki for every mechanic. The passive's
//! party `PROC_TRIGGER_SPELL` aura keeps the payload id in its base points rather than its
//! `EffectTriggerSpell`; the always-procs copy below names it as its trigger spell instead,
//! and writes the main-hand restriction (the `ProcTypeMask` alone names both hands) as the
//! `proc: { hand: mainhand }` override.
//!
//! The wiki's batching timings (400–800 ms uptimes, charges removed a tick later) describe the
//! Classic client's 400 ms spell batching, which the simulator does not model; the charge
//! accounting they lead to is what is tested.

use std::path::Path;
use std::sync::Arc;

use crate::character::context::SwingOutcome;
use crate::character::tests::{Fixture, race, warrior_class};
use crate::character::{Character, SimParams};
use crate::combat_roll::PhysicalAttackResult;
use crate::enchant::{EnchantDb, EnchantName};
use crate::ids::{BuffId, CharId, ProcId, SpellId};
use crate::item::{EquipmentDb, EquipmentSlot, ItemSpec};
use crate::mechanics::Mechanics;
use crate::phase::Phase;
use crate::proc::ProcSource;
use crate::proc::runtime::PROC_ROLL_RANGE;
use crate::race::Race;
use crate::rng::Random;
use crate::spell::overrides::OverrideFile;
use crate::spell::record::{SpellDb, SpellFile};
use crate::spell::{Hand, SpellHost, SpellResult};
use crate::stance::Stance;

/// Windfury Totem Passive (Rank 3): the 20 % proc.
const WINDFURY_PASSIVE: u32 = 10612;
/// The same passive with a 100 % proc chance, for the deterministic mechanics tests.
const WINDFURY_PASSIVE_ALWAYS: u32 = 910612;
/// Windfury Totem (Rank 3): the extra attack and the attack power aura.
const WINDFURY_ATTACK: u32 = 10610;
/// A Sword Specialization style extra attack proc (always fires) and its payload.
const SWORD_SPEC_ALWAYS: u32 = 910001;
const SWORD_SPEC_ATTACK: u32 = 910002;

const WINDFURY_AP: u32 = 246;
const WINDFURY_DURATION: f64 = 1.0;
const WINDFURY_ICD: f64 = 0.1;

const HEROIC_STRIKE: u32 = 78;
const BLOODTHIRST: u32 = 23881;
const SLAM: u32 = 1464;
const WHIRLWIND: u32 = 1680;
const SHIELD_SLAM: u32 = 23925;
const BERSERKER_STANCE: u32 = 2458;
const BERSERKER_STANCE_PASSIVE: u32 = 7381;

const SWORD: u32 = 1;
const DAGGER: u32 = 2;
const SHIELD: u32 = 3;

const WINDFURY_YAML: &str = r#"
build: 1.60.1.70009
spells:
- id: 910612
  name: Windfury Totem Passive (always)
  attributes: [320, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
  school_mask: 8
  levels: {base: 0, spell: 52, max: 60}
  aura_options: {proc_chance: 100, proc_category_recovery_ms: 100, proc_type_mask: 20}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: PROC_TRIGGER_SPELL
    trigger_spell: 10610
    implicit_target: [UNIT_CASTER, NONE]
- id: 910001
  name: Sword Specialization (always)
  attributes: [320, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
  school_mask: 1
  levels: {base: 1, spell: 1}
  aura_options: {proc_chance: 100, proc_type_mask: 4}
  effects:
  - index: 0
    effect: APPLY_AURA
    aura: PROC_TRIGGER_SPELL
    trigger_spell: 910002
    implicit_target: [UNIT_CASTER, NONE]
- id: 910002
  name: Extra Attack
  school_mask: 1
  levels: {base: 1, spell: 1}
  effects:
  - index: 0
    effect: ADD_EXTRA_ATTACKS
    base_points: 1.0
    implicit_target: [UNIT_CASTER, NONE]
"#;

/// The always-procs copy of the passive needs the same two hand-written pieces as the shipped
/// 10612 (`data/spells/overrides/enchants.yaml`): the payload its aura casts, and — because it
/// is learned bare instead of coming from a main-hand enchant, which is what restricts the
/// shipped one — the main hand.
const WINDFURY_OVERRIDES_YAML: &str = r#"
overrides:
  - id: 910612
    note: as if the Windfury Totem enchant sat on the main-hand weapon
    proc: { hand: mainhand }
"#;

/// Fixed-damage weapons so that swing damage is deterministic.
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
  name: Dagger
  phase: 1
  slot: "1H"
  type: DAGGER
  quality: EPIC
  req_lvl: 60
  item_lvl: 60
  damage: { min: 50, max: 50, speed: 1.8 }
- id: 3
  name: Shield
  phase: 1
  slot: OH
  type: SHIELD
  quality: EPIC
  req_lvl: 60
  item_lvl: 60
  damage: { min: 1, max: 1, speed: 1.0 }
  stats: { ARMOR: 2000 }
"#;

/// A dual-wielding Orc Warrior with the shipped spell data plus the Windfury records, every
/// attack roll rigged to a plain hit.
fn fixture() -> Fixture {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let mut f = Fixture::orc_warrior();
    f.db = SpellDb::load(&data.join("spells")).expect("shipped spell data loads");
    let file: SpellFile = serde_yaml::from_str(WINDFURY_YAML).expect("valid Windfury yaml");
    f.db.add_file(file).expect("Windfury records are valid");
    let overrides: OverrideFile =
        serde_yaml::from_str(WINDFURY_OVERRIDES_YAML).expect("valid override yaml");
    f.db.add_overrides(overrides)
        .expect("Windfury overrides are valid");
    f.db.check_references().expect("consistent references");

    let items: Vec<ItemSpec> = serde_yaml::from_str(ITEMS_YAML).unwrap();
    let mut equipment = EquipmentDb::from_specs(items, Vec::new()).unwrap();
    equipment.set_enchants(EnchantDb::load(&data.join("enchants.yaml")).expect("shipped enchants"));
    let equipment = Arc::new(equipment);
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
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    rig_hits(&mut f);
    f
}

/// Every white and yellow roll lands as a plain hit.
fn rig_hits(f: &mut Fixture) {
    f.character
        .roll_mut()
        .random_mut()
        .set_new_range(9999, 10000);
}

/// Learns the Windfury payload and `passive`, enables the proc and returns it.
fn learn_windfury(f: &mut Fixture, passive: u32) -> ProcId {
    f.learn(WINDFURY_ATTACK);
    learn_proc(f, passive)
}

fn learn_proc(f: &mut Fixture, passive: u32) -> ProcId {
    let db = std::mem::take(&mut f.db);
    let added = f.ctx().learn(&db, passive);
    f.db = db;
    let proc = added.proc.expect("the passive is a proc");
    f.ctx().enable_proc(proc);
    proc
}

fn proc_count(f: &Fixture, proc: ProcId) -> u32 {
    f.character.spells().procs().get(proc).procs()
}

fn windfury_buff(f: &Fixture) -> BuffId {
    let spell: SpellId = f.spell_id(WINDFURY_ATTACK);
    f.character
        .spells()
        .spell(spell)
        .marker_buff()
        .expect("Windfury Totem applies an aura")
}

fn buff_active(f: &mut Fixture, buff: BuffId) -> bool {
    f.ctx().buff(buff).is_active()
}

fn buff_charges(f: &mut Fixture, buff: BuffId) -> u32 {
    f.ctx().buff(buff).charges()
}

fn melee_ap(f: &Fixture) -> u32 {
    f.character.melee_ap(&f.target.stat_view())
}

/// Damage of a plain main-hand hit (100 weapon damage at 2.6 speed) with `ap` attack power.
fn mh_hit_damage(f: &Fixture, ap: u32) -> u32 {
    let armor = 1.0 - Mechanics::reduction_from_armor(f.target.armor(), 60);
    ((100.0 + 2.6 * f64::from(ap) / 14.0) * armor).round() as u32
}

/// Damage of a plain off-hand hit (50 weapon damage at 1.8 speed, 50 % penalty).
fn oh_hit_damage(f: &Fixture, ap: u32) -> u32 {
    let armor = 1.0 - Mechanics::reduction_from_armor(f.target.armor(), 60);
    ((50.0 + 1.8 * f64::from(ap) / 14.0) * 0.5 * armor).round() as u32
}

/// Number of main-hand swings performed so far, from the swing scheduling counter (every
/// performed swing and extra attack schedules the next one).
fn mh_swings_scheduled(f: &Fixture) -> u32 {
    f.character.spells().mh_attack().iteration()
}

/// A seed for the proc's `Random` whose first rolls fire (`true`) or not (`false`) at 20 %.
fn seed_for_rolls(pattern: &[bool]) -> u64 {
    (0..100_000u64)
        .find(|&seed| {
            let mut random = Random::from_seed(0, PROC_ROLL_RANGE, seed);
            pattern
                .iter()
                .all(|&fires| (random.get_roll() < 2000) == fires)
        })
        .expect("a seed with that roll pattern")
}

fn set_proc_seed(f: &mut Fixture, proc: ProcId, seed: u64) {
    f.character
        .spells_mut()
        .procs_mut()
        .get_mut(proc)
        .set_seed(seed);
}

// ------------------------------------------------------------------ the enchant

/// "Windfury Totem (WF) is a temporary weapon enchant on your main-hand weapon that give a 20%
/// chance on hit to proc 1 extra attack with bonus attack power." Putting the enchant on the
/// main hand must give the character that proc.
#[test]
fn the_windfury_totem_enchant_on_the_main_hand_registers_its_proc() {
    let mut f = fixture();
    f.set_temp_enchants(EquipmentSlot::Mainhand, &[EnchantName::WindfuryTotem]);
    assert_eq!(
        f.character
            .equipment()
            .temp_enchants(EquipmentSlot::Mainhand),
        [EnchantName::WindfuryTotem]
    );
    let procs = f.character.spells().procs();
    let windfury = procs
        .ids()
        .find(|id| procs.get(*id).name().starts_with("Windfury"));
    assert!(
        windfury.is_some(),
        "equipping the Windfury Totem enchant registers no proc on the character"
    );
    let windfury = windfury.unwrap();
    assert!(procs.is_enabled(windfury), "the enchant proc is enabled");
    assert_eq!(
        procs.get(windfury).sources(),
        &[ProcSource::MainhandSwing, ProcSource::MainhandSpell],
        "a main-hand enchant reacts to main-hand hits only"
    );
    {
        let ctx = f.ctx();
        let procs = ctx.character.spells().procs();
        assert_eq!(
            procs
                .get(windfury)
                .proc_range(ProcSource::MainhandSwing, &ctx),
            2000,
            "20 % on main-hand hits"
        );
    }

    // The payload is on the character and the proc grants it: the extra attack and the aura.
    assert!(f.character.spells().has_game_id(WINDFURY_ATTACK));
    let ap = melee_ap(&f);
    set_proc_seed(&mut f, windfury, seed_for_rolls(&[true]));
    assert_eq!(
        f.ctx().run_proc_checks(&[ProcSource::MainhandSwing]).len(),
        1,
        "the enchant proc fires"
    );
    assert_eq!(melee_ap(&f), ap + WINDFURY_AP);
    assert_eq!(f.character.pending_extra_attacks(), 1);

    // Scraping the enchant off takes the proc out of the checks.
    f.set_temp_enchants(EquipmentSlot::Mainhand, &[]);
    assert!(
        !f.character.spells().procs().is_enabled(windfury),
        "the proc of a removed enchant no longer runs"
    );
}

// ------------------------------------------------------------------ the data

/// The proc is a 20 % chance; its payload grants one extra attack and an aura with two charges
/// that melee swings use up. Attack power and duration follow the Forever tables (246, 1 s);
/// the wiki's Classic values are 315 and 1.5 s.
#[test]
fn windfury_is_a_20_percent_proc_with_a_two_charge_attack_power_aura() {
    let mut f = fixture();
    let proc = learn_windfury(&mut f, WINDFURY_PASSIVE);
    {
        let ctx = f.ctx();
        let procs = ctx.character.spells().procs();
        assert_eq!(
            procs.get(proc).proc_range(ProcSource::MainhandSwing, &ctx),
            2000,
            "20 % per main-hand swing"
        );
        assert_eq!(
            procs.get(proc).proc_range(ProcSource::MainhandSpell, &ctx),
            2000,
            "20 % per melee ability"
        );
    }
    assert_eq!(
        f.character.spells().procs().get(proc).sources(),
        &[
            ProcSource::MainhandSwing,
            ProcSource::OffhandSwing,
            ProcSource::MainhandSpell,
            ProcSource::OffhandSpell
        ],
        "the record reacts to melee swings and melee abilities; the enchant's slot is what          narrows that to the main hand"
    );
    let buff = windfury_buff(&f);
    {
        let ctx = f.ctx();
        let buff = ctx.buff(buff);
        assert_eq!(buff.base_charges(), 2, "two charges");
        assert_eq!(
            buff.charge_sources(),
            &[ProcSource::MainhandSwing, ProcSource::OffhandSwing],
            "each melee swing consumes a charge"
        );
        assert_eq!(buff.duration(), Some(WINDFURY_DURATION), "maximal duration");
    }
    let ap = melee_ap(&f);
    set_proc_seed(&mut f, proc, seed_for_rolls(&[true]));
    let fired = f.ctx().run_proc_checks(&[ProcSource::MainhandSpell]);
    assert_eq!(fired.len(), 1, "the proc fires");
    assert!(f.ctx().aura_active(WINDFURY_ATTACK), "the aura is applied");
    assert_eq!(
        melee_ap(&f),
        ap + WINDFURY_AP,
        "the aura increases attack power"
    );
    assert_eq!(
        f.character.pending_extra_attacks(),
        1,
        "one extra attack is granted"
    );
}

// ------------------------------------------------------------------ proc triggers

/// "Normal melee swings (but not off-hand swings)"
#[test]
fn main_hand_swings_proc_windfury_but_off_hand_swings_do_not() {
    let mut f = fixture();
    let proc = learn_windfury(&mut f, WINDFURY_PASSIVE_ALWAYS);
    f.ctx().start_attack();
    let before = mh_swings_scheduled(&f);

    let outcome = f.ctx().mh_swing(before);
    assert!(matches!(outcome, SwingOutcome::Swing(_)), "{outcome:?}");
    assert_eq!(proc_count(&f, proc), 1, "a main-hand swing procs Windfury");
    assert_eq!(
        mh_swings_scheduled(&f),
        before + 2,
        "the extra attack was performed right away (and rescheduled the swing)"
    );

    // Past the internal cooldown, so only the hand decides.
    f.advance_to(WINDFURY_ICD + 0.01);
    let oh_iteration = f.character.spells().oh_attack().iteration();
    let outcome = f.ctx().oh_swing(oh_iteration);
    assert!(matches!(outcome, SwingOutcome::Swing(_)), "{outcome:?}");
    assert_eq!(
        proc_count(&f, proc),
        1,
        "an off-hand swing does not proc Windfury"
    );
}

/// "On-next-swing attacks (Heroic Strike & Cleave)"
#[test]
fn heroic_strike_procs_windfury() {
    let mut f = fixture();
    let proc = learn_windfury(&mut f, WINDFURY_PASSIVE_ALWAYS);
    f.learn(HEROIC_STRIKE);
    f.set_rage(100);
    let hs = f.spell_id(HEROIC_STRIKE);
    assert!(f.ctx().cast(hs).queued);
    f.ctx().start_attack();
    let iteration = mh_swings_scheduled(&f);
    let outcome = f.ctx().mh_swing(iteration);
    assert!(
        matches!(outcome, SwingOutcome::NextSwingSpell(_)),
        "{outcome:?}"
    );
    assert_eq!(proc_count(&f, proc), 1, "Heroic Strike procs Windfury");
    assert_eq!(
        mh_swings_scheduled(&f),
        iteration + 2,
        "the extra attack was performed right away"
    );
}

/// "Single-target instant attacks"
#[test]
fn bloodthirst_procs_windfury() {
    let mut f = fixture();
    let proc = learn_windfury(&mut f, WINDFURY_PASSIVE_ALWAYS);
    f.learn(BLOODTHIRST);
    f.set_rage(100);
    f.ctx().start_attack();
    let before = mh_swings_scheduled(&f);
    let bt = f.spell_id(BLOODTHIRST);
    let report = f.ctx().cast(bt);
    assert_eq!(report.result, SpellResult::Success);
    assert_eq!(proc_count(&f, proc), 1, "Bloodthirst procs Windfury");
    assert_eq!(
        mh_swings_scheduled(&f),
        before + 1,
        "the extra attack was performed right away"
    );
    assert!(
        f.ctx().aura_active(WINDFURY_ATTACK),
        "one charge is left after the extra attack"
    );
}

/// "Slam (when chain-casting, Improved Slam is required to let the extra attack go off)"
#[test]
fn slam_procs_windfury_when_the_cast_completes() {
    let mut f = fixture();
    let proc = learn_windfury(&mut f, WINDFURY_PASSIVE_ALWAYS);
    f.learn(SLAM);
    f.set_rage(100);
    f.ctx().start_attack();
    let slam = f.spell_id(SLAM);
    let report = f.ctx().cast(slam);
    assert!(report.cast_started);
    assert_eq!(proc_count(&f, proc), 0, "nothing procs on the cast start");
    let handled = f.run(3.0);
    assert!(
        handled
            .iter()
            .any(|kind| matches!(kind, crate::engine::EventKind::CastComplete { .. }))
    );
    assert_eq!(proc_count(&f, proc), 1, "Slam procs Windfury when it lands");
}

/// "Multi-target instant attacks (the proc is on-cast meaning only a single extra attack can be
/// gained even when e.g. Whirlwind hits 4 targets)"
#[test]
fn whirlwind_grants_a_single_extra_attack() {
    let mut f = fixture();
    let proc = learn_windfury(&mut f, WINDFURY_PASSIVE_ALWAYS);
    f.learn(BERSERKER_STANCE);
    f.learn(BERSERKER_STANCE_PASSIVE);
    f.learn(WHIRLWIND);
    let berserker = f.spell_id(BERSERKER_STANCE);
    f.ctx().cast(berserker);
    assert_eq!(f.character.stance(), Stance::Berserker);
    // Off the global cooldown of the stance swap.
    f.advance_to(2.0);
    f.set_rage(100);
    f.ctx().start_attack();
    let before = mh_swings_scheduled(&f);
    let ww = f.spell_id(WHIRLWIND);
    let report = f.ctx().cast(ww);
    assert_eq!(report.result, SpellResult::Success);
    assert_eq!(proc_count(&f, proc), 1, "Whirlwind procs Windfury once");
    assert_eq!(
        mh_swings_scheduled(&f),
        before + 1,
        "exactly one extra attack"
    );
    assert_eq!(f.character.pending_extra_attacks(), 0);
}

/// "Single-target instant attacks (except Shield Slam and Shield Bash; they are considered
/// off-hand attacks)"
#[test]
fn shield_slam_does_not_proc_windfury() {
    let mut f = fixture();
    let proc = learn_windfury(&mut f, WINDFURY_PASSIVE_ALWAYS);
    f.equip(EquipmentSlot::Offhand, SHIELD);
    f.learn(SHIELD_SLAM);
    f.set_rage(100);
    f.ctx().start_attack();
    let before = mh_swings_scheduled(&f);
    let shield_slam = f.spell_id(SHIELD_SLAM);
    let report = f.ctx().cast(shield_slam);
    assert_eq!(report.result, SpellResult::Success);
    assert_eq!(
        proc_count(&f, proc),
        0,
        "Shield Slam is an off-hand attack and does not proc Windfury"
    );
    assert_eq!(mh_swings_scheduled(&f), before, "no extra attack");
}

/// "Other extra attacks (e.g. Ironfoe, Sword Specialization, etc.) but it can't proc itself or
/// proc twice in the same chain of extra attacks"
#[test]
fn extra_attacks_from_other_procs_can_proc_windfury_once_per_chain() {
    let mut f = fixture();
    let windfury = learn_windfury(&mut f, WINDFURY_PASSIVE);
    f.learn(SWORD_SPEC_ATTACK);
    let sword_spec = learn_proc(&mut f, SWORD_SPEC_ALWAYS);
    // Windfury misses the swing itself, procs off Sword Specialization's extra attack, and
    // would proc again off its own extra attack if it were asked.
    set_proc_seed(&mut f, windfury, seed_for_rolls(&[false, true, true]));
    f.ctx().start_attack();
    let before = mh_swings_scheduled(&f);

    let outcome = f.ctx().mh_swing(before);
    assert!(matches!(outcome, SwingOutcome::Swing(_)), "{outcome:?}");
    assert_eq!(
        proc_count(&f, sword_spec),
        1,
        "Sword Specialization procs on the swing"
    );
    assert_eq!(
        proc_count(&f, windfury),
        1,
        "Windfury procs off the Sword Specialization extra attack, and not again off its own"
    );
    assert_eq!(
        mh_swings_scheduled(&f),
        before + 3,
        "the swing, the Sword Specialization extra attack and the Windfury extra attack"
    );
    assert_eq!(f.character.pending_extra_attacks(), 0);
}

/// Windfury never procs off its own extra attack (with the proc at 100 % a chain would
/// otherwise never end).
#[test]
fn windfury_cannot_proc_itself() {
    let mut f = fixture();
    let proc = learn_windfury(&mut f, WINDFURY_PASSIVE_ALWAYS);
    f.ctx().start_attack();
    let before = mh_swings_scheduled(&f);
    let fired = f.ctx().run_proc_checks(&[ProcSource::MainhandSwing]);
    assert_eq!(fired.len(), 1);
    let reports = f.ctx().perform_extra_attacks();
    assert_eq!(reports.len(), 1, "one extra attack, no chain");
    assert_eq!(proc_count(&f, proc), 1);
    assert_eq!(mh_swings_scheduled(&f), before + 1);
    assert_eq!(f.character.pending_extra_attacks(), 0);
}

/// "Windfury Totem, Windfury Weapon, and Wild Strikes have an internal cooldown" — 100 ms in
/// the Forever tables (the wiki's Classic value is 1.5 s).
#[test]
fn windfury_has_an_internal_cooldown() {
    let mut f = fixture();
    let proc = learn_windfury(&mut f, WINDFURY_PASSIVE_ALWAYS);
    f.ctx().start_attack();
    assert_eq!(
        f.ctx().run_proc_checks(&[ProcSource::MainhandSpell]).len(),
        1
    );
    f.ctx().perform_extra_attacks();
    assert!(
        f.ctx()
            .run_proc_checks(&[ProcSource::MainhandSpell])
            .is_empty(),
        "no second proc within the internal cooldown"
    );
    assert_eq!(proc_count(&f, proc), 1);
    f.advance_to(WINDFURY_ICD + 0.001);
    assert_eq!(
        f.ctx().run_proc_checks(&[ProcSource::MainhandSpell]).len(),
        1,
        "the proc is available again after the internal cooldown"
    );
}

// ------------------------------------------------------------------ proc effects

/// Triggered by a normal melee swing: "the two charges are consumed right away (one normal
/// swing + one extra swing)", the swing that proc'd it "does not benefit from the bonus AP" and
/// the extra swing does; the buff is gone before the next regular swing.
#[test]
fn a_swing_proc_uses_both_charges_and_only_the_extra_attack_gets_the_attack_power() {
    let mut f = fixture();
    let proc = learn_windfury(&mut f, WINDFURY_PASSIVE_ALWAYS);
    let buff = windfury_buff(&f);
    let ap = melee_ap(&f);
    f.set_rage(0);
    f.ctx().start_attack();
    let iteration = mh_swings_scheduled(&f);

    let outcome = f.ctx().mh_swing(iteration);
    let SwingOutcome::Swing(report) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(proc_count(&f, proc), 1, "the swing procs Windfury");
    assert_eq!(report.attack.result, PhysicalAttackResult::Hit);
    assert_eq!(
        report.attack.damage,
        mh_hit_damage(&f, ap),
        "the swing that proc'd Windfury does not get the attack power"
    );
    // Rage: both swings land, each 3.46 × 2.6 = 8.996 rage (89 + 90 tenths); the attack power
    // does not change swing rage.
    assert_eq!(
        f.rage(),
        17,
        "the normal swing and the extra attack both give rage"
    );
    assert!(
        !buff_active(&mut f, buff),
        "the normal swing and the extra swing used both charges"
    );
    assert_eq!(
        melee_ap(&f),
        ap,
        "the attack power is gone before the next regular swing"
    );
}

/// Triggered by a melee spell: "one charge is consumed right away by the extra swing"; "an
/// additional melee swing ... will consume a charge, will benefit from the bonus AP" — here the
/// off-hand swing.
#[test]
fn a_spell_proc_leaves_one_charge_for_the_next_swing_which_gets_the_attack_power() {
    let mut f = fixture();
    learn_windfury(&mut f, WINDFURY_PASSIVE_ALWAYS);
    let buff = windfury_buff(&f);
    let ap = melee_ap(&f);
    f.ctx().start_attack();

    // As if an instant attack landed.
    let fired = f.ctx().run_proc_checks(&[ProcSource::MainhandSpell]);
    assert_eq!(fired.len(), 1);
    assert_eq!(buff_charges(&mut f, buff), 2, "fresh aura");
    let reports = f.ctx().perform_extra_attacks();
    assert_eq!(reports.len(), 1, "one extra attack");
    assert_eq!(reports[0].hand, Hand::Mainhand);
    assert_eq!(reports[0].attack.result, PhysicalAttackResult::Hit);
    assert_eq!(
        reports[0].attack.damage,
        mh_hit_damage(&f, ap + WINDFURY_AP),
        "the extra attack benefits from the attack power"
    );
    assert!(buff_active(&mut f, buff));
    assert_eq!(
        buff_charges(&mut f, buff),
        1,
        "the extra attack used one charge"
    );

    // The off-hand swing uses the last charge and benefits from the attack power.
    let oh_iteration = f.character.spells().oh_attack().iteration();
    let outcome = f.ctx().oh_swing(oh_iteration);
    let SwingOutcome::Swing(report) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(report.attack.result, PhysicalAttackResult::Hit);
    assert_eq!(
        report.attack.damage,
        oh_hit_damage(&f, ap + WINDFURY_AP),
        "the off-hand swing benefits from the attack power"
    );
    assert!(
        !buff_active(&mut f, buff),
        "the off-hand swing used the last charge"
    );
    assert_eq!(melee_ap(&f), ap);
}

/// "If no other melee swing occurs after the WF extra swing then the uptime of the buff will be
/// [its maximal duration]" — it expires with a charge left.
#[test]
fn the_aura_expires_after_its_duration_with_a_charge_left() {
    let mut f = fixture();
    learn_windfury(&mut f, WINDFURY_PASSIVE_ALWAYS);
    let buff = windfury_buff(&f);
    let ap = melee_ap(&f);
    f.ctx().start_attack();
    let fired = f.ctx().run_proc_checks(&[ProcSource::MainhandSpell]);
    assert_eq!(fired.len(), 1);
    f.ctx().perform_extra_attacks();
    f.ctx().stop_attack();
    assert!(buff_active(&mut f, buff));
    let applied = f.ctx().buff(buff).applied_at();

    f.run(applied + WINDFURY_DURATION - 0.001);
    assert!(buff_active(&mut f, buff), "still up just before it expires");
    assert_eq!(buff_charges(&mut f, buff), 1, "one charge left");
    f.run(applied + WINDFURY_DURATION + 0.001);
    assert!(!buff_active(&mut f, buff), "expired");
    let expired = f.ctx().buff(buff).expired_at();
    assert!(
        (expired - applied - WINDFURY_DURATION).abs() < 1e-9,
        "uptime {}",
        expired - applied
    );
    assert_eq!(melee_ap(&f), ap);
}

/// "Triggered by melee spell while an on-next-swing attack is queued: ... instead of gaining an
/// extra melee swing the queued attack (Heroic Strike or Cleave) is executed immediately if you
/// have enough rage for it, and it does not consume a charge from the AP buff".
#[test]
fn a_queued_heroic_strike_replaces_the_extra_attack_without_using_a_charge() {
    let mut f = fixture();
    learn_windfury(&mut f, WINDFURY_PASSIVE_ALWAYS);
    f.learn(HEROIC_STRIKE);
    let buff = windfury_buff(&f);
    f.set_rage(100);
    f.ctx().start_attack();
    let hs = f.spell_id(HEROIC_STRIKE);
    assert!(f.ctx().cast(hs).queued);
    assert_eq!(f.character.spells().queued_next_swing(), Some(hs));

    // As if Bloodthirst landed and proc'd Windfury.
    let fired = f.ctx().run_proc_checks(&[ProcSource::MainhandSpell]);
    assert_eq!(fired.len(), 1);
    f.ctx().perform_extra_attacks();

    assert_eq!(
        f.character.spells().queued_next_swing(),
        None,
        "the queued Heroic Strike went off as the extra attack"
    );
    assert_eq!(f.rage(), 85, "Heroic Strike's 15 rage was paid");
    assert!(buff_active(&mut f, buff));
    assert_eq!(
        buff_charges(&mut f, buff),
        2,
        "Heroic Strike is a spell cast and does not use a charge"
    );
}
