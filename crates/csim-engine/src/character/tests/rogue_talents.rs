//! The Rogue's talents on the shipped data: the ones the tables carry completely (stat auras
//! and spell modifiers), checked talent by talent.

use super::energy::{NOTHING, pull, rogue};
use super::rogue::{
    BACKSTAB, EVISCERATE, GHOSTLY_STRIKE, HEMORRHAGE, RUPTURE, SINISTER_STRIKE, cast_at,
    damage_mod, highest_rank, pulled,
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

const HACK_AND_SLASH: u32 = 105727;
const HACK_AND_SLASH_TALENT: u32 = 13960;
const HACK_AND_SLASH_PROC: u32 = 1290312;

/// Equips `item` in `slot` and re-applies the passives its weapon type gates.
fn wield(f: &mut Fixture, slot: EquipmentSlot, item: u32) {
    f.equip(slot, item);
    f.ctx().reevaluate_passives();
}

fn oh_crit(f: &Fixture) -> u32 {
    stat(f, |s, ctx| s.get_oh_crit_chance(ctx))
}

/// Hack and Slash 5/5: +5 % crit for the hand holding a dagger or a fist weapon (an aura crit,
/// suppressed like any other), nothing with other weapons.
#[test]
fn hack_and_slash_crit_with_daggers_and_fist_weapons() {
    for (weapon, crits) in [
        (DAGGER, true),
        (FIST_WEAPON, true),
        (SWORD, false),
        (MACE, false),
        (AXE, false),
    ] {
        let mut base = pulled(&[]);
        let mut f = with_talents(&[(HACK_AND_SLASH, 5)]);
        wield(&mut base, EquipmentSlot::Mainhand, weapon);
        wield(&mut f, EquipmentSlot::Mainhand, weapon);
        if crits {
            base.character.stats_mut().increase_melee_aura_crit(500);
        }
        assert_eq!(mh_crit(&f), mh_crit(&base), "weapon {weapon}");
    }
    let mut base = pulled(&[]);
    let mut f = with_talents(&[(HACK_AND_SLASH, 5)]);
    wield(&mut base, EquipmentSlot::Offhand, DAGGER);
    wield(&mut f, EquipmentSlot::Offhand, DAGGER);
    assert_eq!(mh_crit(&f), mh_crit(&base), "the sword in the main hand");
    base.character.stats_mut().increase_melee_aura_crit(500);
    assert_eq!(oh_crit(&f), oh_crit(&base));
}

/// Hack and Slash: maces ignore 3/6/9/12/15 % of the armor, other weapons nothing.
#[test]
fn hack_and_slash_armor_penetration_with_maces() {
    let mut f = with_talents(&[(HACK_AND_SLASH, 5)]);
    assert_eq!(f.ctx().armor_penetration_percent(Hand::Mainhand), 0);
    wield(&mut f, EquipmentSlot::Mainhand, MACE);
    assert_eq!(f.ctx().armor_penetration_percent(Hand::Mainhand), 15);
    let mut f = with_talents(&[(HACK_AND_SLASH, 2)]);
    wield(&mut f, EquipmentSlot::Mainhand, MACE);
    assert_eq!(f.ctx().armor_penetration_percent(Hand::Mainhand), 6);
}

/// Hack and Slash: the talent is a plain passive; its hidden proc gives swords and axes a
/// 1-5 % chance of an extra attack off swings and abilities, not daggers.
#[test]
fn hack_and_slash_extra_attacks_with_swords_and_axes() {
    let mut f = with_talents(&[(HACK_AND_SLASH, 5)]);
    let spells = f.character.spells();
    assert!(spells.proc_by_game_id(HACK_AND_SLASH_TALENT).is_none());
    let proc = spells
        .proc_by_game_id(HACK_AND_SLASH_PROC)
        .expect("the hidden proc");
    assert!(spells.procs().is_enabled(proc));
    let sources = spells.procs().get(proc).sources().to_vec();
    assert_eq!(
        sources,
        [
            crate::proc::ProcSource::MainhandSwing,
            crate::proc::ProcSource::OffhandSwing,
            crate::proc::ProcSource::MainhandSpell,
            crate::proc::ProcSource::OffhandSpell,
        ]
    );
    let source = crate::proc::ProcSource::MainhandSwing;
    let fulfilled = |f: &mut Fixture| {
        let ctx = f.ctx();
        let procs = ctx.character.spells().procs();
        procs.get(proc).conditions_fulfilled(source, &ctx)
    };
    let range = {
        let ctx = f.ctx();
        ctx.character
            .spells()
            .procs()
            .get(proc)
            .proc_range(source, &ctx)
    };
    assert_eq!(range, 500);
    assert!(fulfilled(&mut f), "sword");
    wield(&mut f, EquipmentSlot::Mainhand, AXE);
    assert!(fulfilled(&mut f), "axe");
    wield(&mut f, EquipmentSlot::Mainhand, DAGGER);
    assert!(!fulfilled(&mut f), "dagger");
    wield(&mut f, EquipmentSlot::Mainhand, SWORD);
    f.ctx().perform_proc(proc);
    assert_eq!(f.character.pending_extra_attacks(), 1);
}

const WEAPON_EXPERTISE: u32 = 105726;

/// Rolls out of 10 000 that the target avoids (miss, dodge, parry) on the character's special
/// and white attack tables, crits aside.
fn avoided_rolls(f: &mut Fixture) -> (u32, u32) {
    use crate::combat_roll::{IncludedOutcomes, ROLL_RANGE};
    use crate::rng::Random;
    let view = f.target.stat_view();
    let ctx = f.character.roll_context(&view);
    let skill = stat(f, |s, stat_ctx| s.get_mh_wpn_skill(stat_ctx));
    let avoided = |result: PhysicalAttackResult| {
        matches!(
            result,
            PhysicalAttackResult::Miss | PhysicalAttackResult::Dodge | PhysicalAttackResult::Parry
        )
    };
    let mut random = Random::new(0, ROLL_RANGE);
    let roll = f.character.roll_mut();
    let special = roll.get_melee_special_table(&ctx, skill).clone();
    let white = roll.get_melee_white_table(&ctx, skill).clone();
    let count = |outcome: &mut dyn FnMut(u32) -> PhysicalAttackResult| {
        (0..ROLL_RANGE).filter(|&r| avoided(outcome(r))).count() as u32
    };
    (
        count(&mut |r| special.get_outcome(&mut random, r, 0, IncludedOutcomes::ALL)),
        count(&mut |r| white.get_outcome(&mut random, r, 0, IncludedOutcomes::ALL)),
    )
}

/// Weapon Expertise 2/2: the target dodges 2 % less, and parries 2 % less when attacked from
/// the front, on every attack table.
#[test]
fn weapon_expertise_lowers_dodge_and_parry() {
    let mut base = pulled(&[]);
    let mut f = with_talents(&[(BLADE_FLURRY, 1), (WEAPON_EXPERTISE, 2)]);
    let (special, white) = avoided_rolls(&mut base);
    assert_eq!(
        avoided_rolls(&mut f),
        (special - 200, white - 200),
        "behind"
    );
    base.character.set_tanking(true);
    f.character.set_tanking(true);
    let (special, white) = avoided_rolls(&mut base);
    assert_eq!(
        avoided_rolls(&mut f),
        (special - 400, white - 400),
        "in front"
    );
}

const QUIETUS: u32 = 110867;

/// Quietus 5/5: Sinister Strike, Ghostly Strike and Hemorrhage deal 10 % more once the target
/// is below 35 % health (the encounter's last 35 %); other abilities do not.
#[test]
fn quietus_raises_the_damage_below_35_percent() {
    let mut f = with_talents(&[(DIRTY_DEEDS, 2), (QUIETUS, 5)]);
    let length = f.character.sim().combat_length;
    for spell in [SINISTER_STRIKE, GHOSTLY_STRIKE, HEMORRHAGE, BACKSTAB] {
        assert!(close(damage_mod(&mut f, spell), 1.0), "{spell}");
    }
    f.advance_to(length * 0.66);
    for spell in [SINISTER_STRIKE, GHOSTLY_STRIKE, HEMORRHAGE] {
        assert!(close(damage_mod(&mut f, spell), 1.1), "{spell}");
    }
    assert!(close(damage_mod(&mut f, BACKSTAB), 1.0));
}

const VILE_POISONS: u32 = 105714;
const IMPROVED_POISONS: u32 = 105713;
const VENOM: u32 = 105712;
const VENOM_SPELL: u32 = 1310703;

/// The poison modifiers of a character: damage % of the poisons (Instant 8192 and Deadly
/// `[_, 8]`), Deadly Poison's periodic damage % (65536) and the flat chance to apply them.
fn poison_modifiers(f: &Fixture) -> (f64, f64, f64) {
    use crate::spell::dbc::SpellModOp;
    use crate::spell::record::ClassOptions;
    let modifiers = f.character.spell_modifiers();
    let instant = ClassOptions {
        set: 8,
        mask: [8192, 0, 0, 0],
    };
    let deadly = ClassOptions {
        set: 8,
        mask: [65536, 8, 0, 0],
    };
    (
        modifiers.pct(Some(&instant), SpellModOp::HealingAndDamage),
        modifiers.pct(Some(&deadly), SpellModOp::PeriodicHealingAndDamage),
        modifiers.flat(Some(&instant), SpellModOp::ProcChance),
    )
}

/// Vile Poisons 5/5 (+20 % damage, +20 % Deadly Poison ticks) and Improved Poisons 5/5 (+10 %
/// chance) modify the poisons (see `rogue_poisons` for their effect on the poisons).
#[test]
fn vile_and_improved_poisons_modify_the_poisons() {
    let f = with_talents(&[(VILE_POISONS, 5), (IMPROVED_POISONS, 5)]);
    assert_eq!(poison_modifiers(&f), (20.0, 20.0, 10.0));
}

/// Venom: a finisher whose buff lasts 6 s plus 3 s per combo point and, while it lasts, adds
/// 30 % poison damage, 30 % to Deadly Poison's ticks and 10 % chance to apply poisons.
#[test]
fn venom_is_a_finisher_that_empowers_the_poisons() {
    let mut f = with_talents(&[(MUTILATE, 1), (VENOM, 1)]);
    assert_eq!(poison_modifiers(&f), (0.0, 0.0, 0.0));
    assert_eq!(f.status(VENOM_SPELL), SpellStatus::InsufficientComboPoints);
    super::rogue::set_combo_points(&mut f, 5);
    let report = cast_at(&mut f, VENOM_SPELL, 0.0);
    assert_eq!(report.combo_points_spent, 5);
    assert_eq!(super::rogue::duration(&mut f, VENOM_SPELL), Some(21.0));
    assert_eq!(poison_modifiers(&f), (30.0, 30.0, 10.0));
    f.advance_to(21.5);
    assert_eq!(poison_modifiers(&f), (0.0, 0.0, 0.0));
    super::rogue::set_combo_points(&mut f, 1);
    cast_at(&mut f, VENOM_SPELL, 22.0);
    assert_eq!(super::rogue::duration(&mut f, VENOM_SPELL), Some(9.0));
}
