//! The rogue poisons on the shipped data: the `SpellItemEnchantment` rows of Instant Poison VI
//! and Deadly Poison V as procs of the hand they coat, their chance modifiers (Improved
//! Poisons), attack power scaling and damage modifiers (Vile Poisons), Deadly Poison's stacks
//! and Mutilate's bonus against a poisoned target.

use super::rogue::{cast_at, highest_rank};
use super::rogue_talents::with_talents;
use super::*;
use crate::character_spells::{EquipmentGrantor, SpellHandle};
use crate::effect::EffectHost;
use crate::ids::ProcId;
use crate::proc::ProcSource;
use crate::spell::CastReport;

const INSTANT_POISON: u32 = 11337;
const DEADLY_POISON: u32 = 25349;

const IMPROVED_POISONS: u32 = 105713;
const VILE_POISONS: u32 = 105714;
const MUTILATE: u32 = 105709;

/// A rogue with `talents`, a sword with Instant Poison in the main hand and a dagger with
/// Deadly Poison in the off hand, against a level 60 target (no level-based resistance), every
/// roll rigged to a hit.
fn poisoned(talents: &[(u32, u32)]) -> Fixture {
    let mut f = with_talents(talents);
    f.target.set_level(60);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    f.set_temp_enchants(EquipmentSlot::Mainhand, &[EnchantName::InstantPoison]);
    f.set_temp_enchants(EquipmentSlot::Offhand, &[EnchantName::DeadlyPoison]);
    f
}

/// The poison proc of the enchant in `slot`.
fn poison_proc(f: &Fixture, slot: EquipmentSlot) -> ProcId {
    f.character
        .spells()
        .equipment_spells()
        .find_map(|(key, handle)| match (key.slot, key.grantor, handle) {
            (Some(s), EquipmentGrantor::Enchant(_), SpellHandle::Proc(id)) if s == slot => Some(id),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a poison on {slot:?}"))
}

/// The chance (out of 10 000) of the poison in `slot` on an event from `source`.
fn chance(f: &mut Fixture, slot: EquipmentSlot, source: ProcSource) -> u32 {
    let id = poison_proc(f, slot);
    let ctx = f.ctx();
    ctx.character
        .spells()
        .procs()
        .get(id)
        .proc_range(source, &ctx)
}

/// Fires the poison in `slot` once and returns the payload's report.
fn fire(f: &mut Fixture, slot: EquipmentSlot) -> CastReport {
    let id = poison_proc(f, slot);
    let mut report = f.ctx().perform_proc(id);
    assert_eq!(report.triggered.len(), 1);
    report.triggered.remove(0).1
}

fn melee_ap(f: &mut Fixture) -> f64 {
    f64::from(f.ctx().melee_ap())
}

fn deadly_poison_buff(f: &Fixture) -> crate::ids::BuffId {
    super::rogue::buff_of(f, DEADLY_POISON)
}

/// Both hands proc their own poison off their swings and abilities, at the enchantment's
/// chance (Instant 20 %, Deadly 30 %); the proc casts the learned payload.
#[test]
fn poisons_proc_off_their_own_hand_at_the_enchantment_chance() {
    let mut f = poisoned(&[]);
    let spells = f.character.spells();
    let sources = |slot| spells.procs().get(poison_proc(&f, slot)).sources().to_vec();
    assert_eq!(
        sources(EquipmentSlot::Mainhand),
        [ProcSource::MainhandSwing, ProcSource::MainhandSpell]
    );
    assert_eq!(
        sources(EquipmentSlot::Offhand),
        [ProcSource::OffhandSwing, ProcSource::OffhandSpell]
    );
    assert!(spells.spell_by_game_id(INSTANT_POISON).is_some());
    assert!(spells.spell_by_game_id(DEADLY_POISON).is_some());
    let mh = (EquipmentSlot::Mainhand, ProcSource::MainhandSwing);
    let oh = (EquipmentSlot::Offhand, ProcSource::OffhandSpell);
    assert_eq!(chance(&mut f, mh.0, mh.1), 2000);
    assert_eq!(chance(&mut f, oh.0, oh.1), 3000);

    // Scraping the poison off takes its proc away.
    f.set_temp_enchants(EquipmentSlot::Offhand, &[]);
    let procs = f.character.spells().procs();
    assert!(
        procs
            .enabled()
            .iter()
            .all(|&id| procs.get(id).game_id() != DEADLY_POISON)
    );
}

/// Instant Poison on both hands is one learned spell; it stays enabled while either hand has
/// the poison.
#[test]
fn both_hands_share_the_payload() {
    let mut f = poisoned(&[]);
    f.set_temp_enchants(EquipmentSlot::Offhand, &[EnchantName::InstantPoison]);
    let instant = f.spell_id(INSTANT_POISON);
    let enabled = |f: &Fixture| f.character.spells().spell(instant).is_enabled();
    assert_ne!(
        poison_proc(&f, EquipmentSlot::Mainhand),
        poison_proc(&f, EquipmentSlot::Offhand)
    );
    assert!(enabled(&f));
    f.set_temp_enchants(EquipmentSlot::Mainhand, &[]);
    assert!(enabled(&f), "the off hand still has it");
    f.set_temp_enchants(EquipmentSlot::Offhand, &[]);
    assert!(!enabled(&f));
}

/// Improved Poisons 5/5 adds 10 % to the chance of both poisons.
#[test]
fn improved_poisons_raises_the_chance() {
    let mut f = poisoned(&[(IMPROVED_POISONS, 5)]);
    let mh = chance(&mut f, EquipmentSlot::Mainhand, ProcSource::MainhandSpell);
    let oh = chance(&mut f, EquipmentSlot::Offhand, ProcSource::OffhandSwing);
    assert_eq!((mh, oh), (3000, 4000));
}

/// Poison procs over many main-hand swings land near the chance.
#[test]
fn instant_poison_procs_a_fifth_of_the_swings() {
    let mut f = poisoned(&[]);
    let id = poison_proc(&f, EquipmentSlot::Mainhand);
    let checks = 20_000;
    for _ in 0..checks {
        f.ctx().run_proc_checks(&[ProcSource::MainhandSwing]);
    }
    let procs = f.character.spells().procs().get(id).procs();
    let rate = f64::from(procs) / f64::from(checks);
    assert!((rate - 0.2).abs() < 0.01, "{rate}");
}

/// Instant Poison VI: 88 ± 13.8 % nature damage on the magic table plus 0.5 % of attack power;
/// Vile Poisons 5/5 adds 20 %.
#[test]
fn instant_poison_deals_nature_damage_with_attack_power() {
    for (talents, multiplier) in [(vec![], 1.0), (vec![(VILE_POISONS, 5)], 1.2)] {
        let mut f = poisoned(&talents);
        let ap = melee_ap(&mut f);
        let (low, high) = (
            88.0 * (1.0 - 0.2769231 / 2.0),
            88.0 * (1.0 + 0.2769231 / 2.0),
        );
        let (min, max) = (
            ((low + ap * 0.005) * multiplier).floor(),
            ((high + ap * 0.005) * multiplier).ceil(),
        );
        for _ in 0..50 {
            let report = fire(&mut f, EquipmentSlot::Mainhand);
            let attack = report.attack.expect("the poison rolled");
            let spell = attack.spell.expect("on the magic table");
            assert_eq!(spell.resisted, 0);
            let damage = f64::from(attack.damage);
            assert!(
                (min..=max).contains(&damage),
                "{damage} not in {min}..{max} ({multiplier})"
            );
        }
    }
}

/// Deadly Poison V: a debuff of up to 5 stacks, each application adds one and refreshes the
/// 12 s; every 3 s it deals 23 plus 0.45 % of attack power per stack, 20 % more with Vile
/// Poisons 5/5.
#[test]
fn deadly_poison_stacks_to_five_and_refreshes() {
    let mut f = poisoned(&[(VILE_POISONS, 5)]);
    let ap = melee_ap(&mut f);
    let buff = deadly_poison_buff(&f);
    for (time, stacks) in [(0.0, 1), (0.5, 2), (1.0, 3), (1.5, 4), (2.0, 5), (2.5, 5)] {
        f.advance_to(time);
        let report = fire(&mut f, EquipmentSlot::Offhand);
        assert!(report.buff.is_some(), "applied at {time}");
        let buff = f.ctx().buff_ref(buff).clone();
        assert_eq!(buff.stacks(), stacks, "at {time}");
        assert!(
            (buff.time_left(time) - 12.0).abs() < 1e-9,
            "refreshed at {time}"
        );
    }
    assert!(f.ctx().target_poisoned_by_caster());

    // The first tick, 3 s after the first application, with all five stacks.
    f.ctx().take_statistics();
    f.advance_to(3.01);
    let stats = f.ctx().take_statistics();
    let rank = f.character.spells().spell(f.spell_id(DEADLY_POISON)).rank();
    let deadly = stats
        .spell_statistics("Deadly Poison V", rank)
        .expect("Deadly Poison ticked");
    let expected = ((23.0 + ap * 0.0045) * 5.0 * 1.2).round() as u64;
    assert_eq!(deadly.total_damage(), expected);

    // Four ticks after the last refresh the stacks are gone.
    f.advance_to(2.5 + 12.01);
    assert!(!f.ctx().buff_ref(buff).is_active());
    assert!(!f.ctx().target_poisoned_by_caster());
}

/// A rogue's Deadly Poison is their own: not a raid-wide debuff shared by several rogues.
#[test]
fn deadly_poison_is_not_shared_by_the_raid() {
    let f = poisoned(&[]);
    let buff = deadly_poison_buff(&f);
    let mut f = f;
    assert_ne!(
        f.ctx().buff_ref(buff).kind(),
        crate::buff::BuffKind::SharedDebuff
    );
}

/// Mutilate's strikes deal 20 % more damage while one of the rogue's poisons (Deadly Poison,
/// a debuff) is on the target.
#[test]
fn mutilate_hits_harder_against_a_poisoned_target() {
    let mut f = poisoned(&[(MUTILATE, 1)]);
    // Daggers whose damage does not vary, so the strikes compare; the poisons come off the
    // weapons so that none lands on its own, and Deadly Poison is cast by hand (its learned
    // spell went off with the proc).
    f.equip(EquipmentSlot::Mainhand, EVEN_DAGGER);
    f.equip(EquipmentSlot::Offhand, EVEN_DAGGER);
    f.set_temp_enchants(EquipmentSlot::Mainhand, &[]);
    f.set_temp_enchants(EquipmentSlot::Offhand, &[]);
    let deadly = f.spell_id(DEADLY_POISON);
    assert!(!f.character.spells().spell(deadly).is_enabled());
    f.ctx().enable_spell(deadly);
    f.target.set_base_armor(0);
    let mutilate = highest_rank(&f, "Mutilate");
    let game_id = f.character.spells().spell(mutilate).game_id();
    let strikes = |report: &CastReport| -> Vec<u32> {
        report
            .triggered
            .iter()
            .map(|(_, strike)| strike.attack.unwrap().damage)
            .collect()
    };

    let clean = cast_at(&mut f, game_id, 0.0);
    assert!(!f.ctx().target_poisoned_by_caster());
    f.ctx().trigger_spell(DEADLY_POISON, None);
    assert!(f.ctx().target_poisoned_by_caster());
    let poisoned = cast_at(&mut f, game_id, 3.0);
    assert_eq!(strikes(&clean).len(), 2);
    for (clean, poisoned) in strikes(&clean).into_iter().zip(strikes(&poisoned)) {
        let ratio = f64::from(poisoned) / f64::from(clean);
        assert!((ratio - 1.2).abs() < 0.02, "{poisoned} / {clean}");
    }
}
