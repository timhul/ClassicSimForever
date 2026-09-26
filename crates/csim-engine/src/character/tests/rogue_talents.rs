//! The Rogue's talents on the shipped data: the ones the tables carry completely (stat auras
//! and spell modifiers), checked talent by talent.

use super::energy::{NOTHING, pull, rogue};
use super::rogue::{
    BACKSTAB, EVISCERATE, RUPTURE, SINISTER_STRIKE, cast_at, damage_mod, highest_rank, pulled,
};
use super::*;
use crate::magic_school::MagicSchool;
use crate::target::CreatureType;

const MALICE: u32 = 105722;
const MURDER: u32 = 105720;
const LETHALITY: u32 = 105716;
const COLD_BLOOD: u32 = 105715;
const IMPROVED_EVISCERATE: u32 = 105708;
const IMPROVED_SINISTER_STRIKE: u32 = 105741;
const PUNCTURING_WOUNDS: u32 = 105719;
const PRECISION: u32 = 105737;
const FLAWLESS_EXECUTION: u32 = 108100;
const DUAL_WIELD_SPECIALIZATION: u32 = 105740;
const BLADE_FLURRY: u32 = 105728;
const AGGRESSION: u32 = 105730;
const OPPORTUNITY: u32 = 105760;
const IMPROVED_AMBUSH: u32 = 105749;
const SERRATED_BLADES: u32 = 105752;
const DIRTY_DEEDS: u32 = 105745;
const MUTILATE: u32 = 105709;

const GARROTE: u32 = 11290;
const COLD_BLOOD_SPELL: u32 = 14177;
const BLADE_FLURRY_SPELL: u32 = 13877;

/// A rogue at the pull with `talents` forced in (the tiers above them need no points; a
/// prerequisite must come before the talent that needs it), every roll rigged to a hit.
pub(super) fn with_talents(talents: &[(u32, u32)]) -> Fixture {
    let mut f = rogue(&[], NOTHING);
    for &(node, rank) in talents {
        for _ in 0..rank {
            let change = f
                .character
                .talents_mut()
                .and_then(|t| t.force_increment_rank(node))
                .unwrap_or_else(|| panic!("a point in {node}"));
            f.ctx().apply_talent_changes([change]);
        }
    }
    f.ctx().prepare_set_of_combat_iterations();
    f.rig_rolls(PhysicalAttackResult::Hit);
    pull(&mut f);
    f
}

fn stat<R>(
    f: &Fixture,
    read: impl FnOnce(&crate::stats::CharacterStats, &crate::stats::StatContext) -> R,
) -> R {
    let view = f.target.stat_view();
    read(f.character.stats(), &f.character.stat_context(&view))
}

fn mh_crit(f: &Fixture) -> u32 {
    stat(f, |s, ctx| s.get_mh_crit_chance(ctx))
}

fn phys_dmg_mod(f: &Fixture) -> f64 {
    stat(f, |s, ctx| s.get_total_physical_damage_mod(ctx))
}

fn cost(f: &mut Fixture, game_id: u32) -> u32 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).resource_cost(&ctx)
}

fn crit_bonus(f: &mut Fixture, spell: SpellId) -> u32 {
    let ctx = f.ctx();
    ctx.character.spells().spell(spell).crit_chance_bonus(&ctx)
}

fn crit_damage_mod(f: &mut Fixture, spell: SpellId) -> f64 {
    let ctx = f.ctx();
    ctx.character.spells().spell(spell).crit_damage_mod(&ctx)
}

fn periodic_damage_mod(f: &mut Fixture, game_id: u32) -> f64 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).periodic_damage_mod(&ctx)
}

fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() < 1e-9
}

/// Malice: +5 % crit with every attack (an aura crit, suppressed against a boss like any
/// other) and with poisons (the spell crit).
#[test]
fn malice_raises_melee_and_spell_crit() {
    let mut base = pulled(&[]);
    let mut f = with_talents(&[(MALICE, 5)]);
    let spell_crit = |f: &Fixture| {
        stat(f, |s, ctx| {
            s.get_spell_crit_chance(ctx, MagicSchool::Nature)
        })
    };
    assert_eq!(spell_crit(&f), spell_crit(&base) + 500);
    assert!(mh_crit(&f) > mh_crit(&base));
    base.character.stats_mut().increase_melee_aura_crit(500);
    assert_eq!(mh_crit(&f), mh_crit(&base));
    let ss = f.spell_id(SINISTER_STRIKE);
    assert_eq!(crit_bonus(&mut f, ss), 0, "an aura, not a modifier");
}

/// Murder: +4 % damage against humanoids and giants, nothing against the default dragonkin.
#[test]
fn murder_raises_damage_against_humanoids_and_giants() {
    let base = pulled(&[]);
    let mut f = with_talents(&[(MURDER, 2)]);
    assert_eq!(f.target.creature_type(), CreatureType::Dragonkin);
    assert!(close(phys_dmg_mod(&f), phys_dmg_mod(&base)));
    for creature in [CreatureType::Humanoid, CreatureType::Giant] {
        f.target.set_creature_type(creature);
        assert!(
            close(phys_dmg_mod(&f), phys_dmg_mod(&base) * 1.04),
            "{creature:?}"
        );
        let magic = stat(&f, |s, ctx| {
            s.get_magic_school_damage_mod(ctx, MagicSchool::Nature)
        });
        assert!(close(magic, 1.04), "{creature:?}: {magic}");
    }
    f.target.set_creature_type(CreatureType::Undead);
    assert!(close(phys_dmg_mod(&f), phys_dmg_mod(&base)));
}

/// Lethality: the crit bonus of Sinister Strike, Backstab, Ghostly Strike, Hemorrhage and
/// Mutilate's strikes grows by 20 %, Eviscerate's does not.
#[test]
fn lethality_raises_the_crit_bonus_of_the_builders() {
    let mut f = with_talents(&[(MALICE, 5), (LETHALITY, 5), (MUTILATE, 1)]);
    let ss = f.spell_id(SINISTER_STRIKE);
    let backstab = f.spell_id(BACKSTAB);
    let eviscerate = f.spell_id(EVISCERATE);
    let strike = f.spell_id(1310706);
    for spell in [ss, backstab, strike] {
        assert!(close(crit_damage_mod(&mut f, spell), 2.2), "{spell:?}");
    }
    assert!(close(crit_damage_mod(&mut f, eviscerate), 2.0));
}

/// Improved Eviscerate (+20 %) and Aggression (+6 %) add up on Eviscerate; Aggression alone on
/// Sinister Strike and Backstab.
#[test]
fn improved_eviscerate_and_aggression_raise_the_damage() {
    let mut f = with_talents(&[(IMPROVED_EVISCERATE, 3), (AGGRESSION, 3)]);
    assert!(close(damage_mod(&mut f, EVISCERATE), 1.26));
    assert!(close(damage_mod(&mut f, SINISTER_STRIKE), 1.06));
    assert!(close(damage_mod(&mut f, BACKSTAB), 1.06));
    assert!(close(damage_mod(&mut f, RUPTURE), 1.0));
}

/// Improved Sinister Strike (−5) and Flawless Execution (−10) lower the energy costs.
#[test]
fn cost_reductions() {
    let mut base = pulled(&[]);
    let mut f = with_talents(&[
        (IMPROVED_SINISTER_STRIKE, 2),
        (FLAWLESS_EXECUTION, 1),
        (DIRTY_DEEDS, 2),
    ]);
    assert_eq!(cost(&mut base, SINISTER_STRIKE), 45);
    assert_eq!(cost(&mut f, SINISTER_STRIKE), 40);
    assert_eq!(cost(&mut base, EVISCERATE), 35);
    assert_eq!(cost(&mut f, EVISCERATE), 25);
    assert_eq!(cost(&mut base, GARROTE), 50);
    assert_eq!(cost(&mut f, GARROTE), 30);
    assert_eq!(cost(&mut f, BACKSTAB), 60);
}

/// Precision: +3 % melee and spell hit.
#[test]
fn precision_raises_melee_and_spell_hit() {
    let base = pulled(&[]);
    let f = with_talents(&[(PRECISION, 3)]);
    let melee = |f: &Fixture| stat(f, |s, ctx| s.get_melee_hit_chance(ctx));
    let spell = |f: &Fixture| stat(f, |s, ctx| s.get_spell_hit_chance(ctx, MagicSchool::Nature));
    assert_eq!(melee(&f), melee(&base) + 300);
    assert_eq!(spell(&f), spell(&base) + 300);
}

/// Dual Wield Specialization: the off hand deals 50 % + 25 % of 50 % more.
#[test]
fn dual_wield_specialization_raises_the_off_hand_damage() {
    let f = with_talents(&[(PRECISION, 3), (DUAL_WIELD_SPECIALIZATION, 5)]);
    let penalty = f.character.spells().oh_attack().offhand_penalty();
    assert!(close(penalty, 0.625), "{penalty}");
}

/// Opportunity: Backstab, Ambush and Mutilate +10 % damage, Garrote's ticks +10 %.
#[test]
fn opportunity_raises_the_openers_and_backstab() {
    let mut f = with_talents(&[(OPPORTUNITY, 2)]);
    assert!(close(damage_mod(&mut f, BACKSTAB), 1.1));
    let ambush = highest_rank(&f, "Ambush");
    let ambush = f.character.spells().spell(ambush).game_id();
    assert!(close(damage_mod(&mut f, ambush), 1.1));
    assert!(close(periodic_damage_mod(&mut f, GARROTE), 1.1));
    assert!(close(damage_mod(&mut f, SINISTER_STRIKE), 1.0));
}

/// Improved Ambush +45 % crit on Ambush; Puncturing Wounds +30 % on Backstab and +15 % on
/// Mutilate's strikes.
#[test]
fn crit_chance_modifiers() {
    let mut f = with_talents(&[(IMPROVED_AMBUSH, 3), (PUNCTURING_WOUNDS, 3), (MUTILATE, 1)]);
    let ambush = highest_rank(&f, "Ambush");
    assert_eq!(crit_bonus(&mut f, ambush), 4500);
    let backstab = f.spell_id(BACKSTAB);
    assert_eq!(crit_bonus(&mut f, backstab), 3000);
    for strike in [1310706, 1310705] {
        let strike = f.spell_id(strike);
        assert_eq!(crit_bonus(&mut f, strike), 1500);
    }
    let ss = f.spell_id(SINISTER_STRIKE);
    assert_eq!(crit_bonus(&mut f, ss), 0);
}

/// Serrated Blades: every weapon ignores 9 % of the armor, Rupture's ticks +30 %.
#[test]
fn serrated_blades_penetrates_armor_and_raises_rupture() {
    let mut f = with_talents(&[(SERRATED_BLADES, 3)]);
    for (slot, item) in [
        (EquipmentSlot::Mainhand, SWORD),
        (EquipmentSlot::Mainhand, DAGGER),
    ] {
        f.equip(slot, item);
        assert_eq!(f.ctx().armor_penetration_percent(Hand::Mainhand), 9);
    }
    assert!(close(periodic_damage_mod(&mut f, RUPTURE), 1.3));
    assert!(close(periodic_damage_mod(&mut f, GARROTE), 1.0));
}

/// Blade Flurry: +20 % attack speed for 15 s.
#[test]
fn blade_flurry_hastes_the_attacks() {
    let mut f = with_talents(&[(BLADE_FLURRY, 1)]);
    let speed = f.character.stats().get_melee_attack_speed_mod();
    cast_at(&mut f, BLADE_FLURRY_SPELL, 0.0);
    let hasted = f.character.stats().get_melee_attack_speed_mod();
    assert!(close(hasted / speed, 1.2), "{speed} -> {hasted}");
    f.advance_to(15.5);
    assert!(close(
        f.character.stats().get_melee_attack_speed_mod(),
        speed
    ));
}

/// Cold Blood: the next Sinister Strike, Backstab, Ambush, Eviscerate or Mutilate crits, and
/// uses the buff up; other abilities leave it.
#[test]
fn cold_blood_makes_the_next_strike_crit() {
    let mut f = with_talents(&[(COLD_BLOOD, 1)]);
    cast_at(&mut f, COLD_BLOOD_SPELL, 0.0);
    assert!(f.ctx().aura_active(COLD_BLOOD_SPELL));
    let ss = f.spell_id(SINISTER_STRIKE);
    assert_eq!(crit_bonus(&mut f, ss), 10000);
    let report = cast_at(&mut f, SINISTER_STRIKE, 0.0);
    assert_eq!(
        report.attack.unwrap().result,
        PhysicalAttackResult::Critical
    );
    assert!(!f.ctx().aura_active(COLD_BLOOD_SPELL));
    let report = cast_at(&mut f, SINISTER_STRIKE, 1.0);
    assert_eq!(report.attack.unwrap().result, PhysicalAttackResult::Hit);
}

/// Cold Blood with Mutilate: both strikes crit (the modifier selects the strikes, not
/// Mutilate itself) and the cast uses the buff up once.
#[test]
fn cold_blood_is_used_by_mutilates_strikes() {
    let mut f = with_talents(&[(COLD_BLOOD, 1), (MUTILATE, 1)]);
    f.equip(EquipmentSlot::Mainhand, DAGGER);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    let mutilate = highest_rank(&f, "Mutilate");
    let mutilate = f.character.spells().spell(mutilate).game_id();
    cast_at(&mut f, COLD_BLOOD_SPELL, 0.0);
    let report = cast_at(&mut f, mutilate, 0.0);
    assert_eq!(report.triggered.len(), 2);
    for (_, strike) in &report.triggered {
        assert_eq!(
            strike.attack.unwrap().result,
            PhysicalAttackResult::Critical
        );
    }
    assert!(!f.ctx().aura_active(COLD_BLOOD_SPELL));
}
