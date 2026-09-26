//! The Rogue's combo point builders and finishers on the shipped data: combo points, the
//! finishers' damage, durations and aura values by the points they spend, the weapon and
//! position requirements, Hemorrhage's damage taken from the caster and Mutilate's strikes.

use super::energy::{NOTHING, energy, pull, rogue};
use super::*;
use crate::effect::EffectHost;
use crate::ids::BuffId;
use crate::mechanics::Mechanics;
use crate::spell::CastReport;
use crate::statistics::SpellKey;

pub(super) const SINISTER_STRIKE: u32 = 11294;
pub(super) const BACKSTAB: u32 = 25300;
pub(super) const EVISCERATE: u32 = 31016;
pub(super) const SLICE_AND_DICE: u32 = 6774;
pub(super) const RUPTURE: u32 = 11275;
pub(super) const EXPOSE_ARMOR: u32 = 11198;
pub(super) const GHOSTLY_STRIKE: u32 = 14278;
pub(super) const HEMORRHAGE: u32 = 16511;

/// Malice 5/5 and Improved Slice and Dice 3/3 (+45 % duration).
const IMPROVED_SLICE_AND_DICE: [(u32, u32); 2] = [(105722, 5), (105739, 3)];

/// Assassination up to Mutilate, around Ruthlessness.
const TO_MUTILATE: [(u32, u32); 6] = [
    (105722, 5),
    (105720, 2),
    (105739, 3),
    (105716, 5),
    (105714, 5),
    (105709, 1),
];

/// A rogue at the pull that does nothing on its own, every roll rigged to a hit.
pub(super) fn pulled(talents: &[(u32, u32)]) -> Fixture {
    let mut f = rogue(talents, NOTHING);
    f.rig_rolls(PhysicalAttackResult::Hit);
    pull(&mut f);
    f
}

pub(super) fn combo_points(f: &mut Fixture) -> u32 {
    f.ctx().combo_points()
}

pub(super) fn set_combo_points(f: &mut Fixture, points: u32) {
    let mut ctx = f.ctx();
    ctx.spend_combo_points();
    ctx.gain_combo_points(points);
}

/// Casts `game_id` at `time`, past the global cooldown of the previous cast.
pub(super) fn cast_at(f: &mut Fixture, game_id: u32, time: f64) -> CastReport {
    f.advance_to(time);
    assert_eq!(
        f.status(game_id),
        SpellStatus::Available,
        "{game_id} at {time}"
    );
    let id = f.spell_id(game_id);
    f.ctx().cast(id)
}

pub(super) fn buff_of(f: &Fixture, game_id: u32) -> BuffId {
    f.character
        .spells()
        .spell(f.spell_id(game_id))
        .marker_buff()
        .expect("the spell has an aura")
}

pub(super) fn duration(f: &mut Fixture, game_id: u32) -> Option<f64> {
    let buff = buff_of(f, game_id);
    f.ctx().buff_ref(buff).duration()
}

pub(super) fn highest_rank(f: &Fixture, name: &str) -> SpellId {
    let spells = f.character.spells();
    spells
        .rank_group(name)
        .and_then(|group| group.get_max_available_spell_rank(|id| spells.spell(id).is_enabled()))
        .unwrap_or_else(|| panic!("{name} has an enabled rank"))
}

/// The value of effect `index` of `game_id` as the character sees it now.
pub(super) fn effect_value(f: &mut Fixture, game_id: u32, index: u32) -> f64 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    let effect = ctx
        .character
        .spells()
        .spell(id)
        .effects()
        .iter()
        .find(|e| e.index() == index)
        .expect("a direct effect")
        .clone();
    effect.effective_value(&ctx)
}

pub(super) fn damage_mod(f: &mut Fixture, game_id: u32) -> f64 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).damage_mod(&ctx)
}

#[test]
fn builders_award_combo_points() {
    let mut f = pulled(&[]);
    assert_eq!(combo_points(&mut f), 0);
    let report = cast_at(&mut f, SINISTER_STRIKE, 0.0);
    assert_eq!(report.result, SpellResult::Success);
    assert_eq!(report.combo_points_spent, 0);
    assert_eq!(combo_points(&mut f), 1);
    assert_eq!(energy(&f, 0.0), 55);
    cast_at(&mut f, SINISTER_STRIKE, 1.0);
    assert_eq!(combo_points(&mut f), 2);
}

#[test]
fn a_finisher_needs_a_combo_point() {
    let mut f = pulled(&[]);
    assert_eq!(f.status(EVISCERATE), SpellStatus::InsufficientComboPoints);
    set_combo_points(&mut f, 1);
    assert_eq!(f.status(EVISCERATE), SpellStatus::Available);
}

#[test]
fn backstab_needs_a_main_hand_dagger_and_the_back_of_the_target() {
    let mut f = pulled(&[]);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    assert_eq!(
        f.status(BACKSTAB),
        SpellStatus::IncorrectWeaponType,
        "a dagger in the off hand is not enough"
    );
    f.equip(EquipmentSlot::Mainhand, DAGGER);
    assert_eq!(f.status(BACKSTAB), SpellStatus::Available);
    f.character.set_tanking(true);
    assert_eq!(f.status(BACKSTAB), SpellStatus::NotBehindTarget);
    assert_eq!(f.status(SINISTER_STRIKE), SpellStatus::Available);
    f.character.set_tanking(false);
    let report = cast_at(&mut f, BACKSTAB, 0.0);
    assert_eq!(report.result, SpellResult::Success);
    assert_eq!(combo_points(&mut f), 1);
}

/// Eviscerate r9: 108 ± 50 % plus 170 and 3 % of attack power per point, spent after the
/// damage is dealt. The target has no armor, so the damage is the raw damage.
#[test]
fn eviscerate_damage_grows_with_combo_points_and_attack_power() {
    let mut f = pulled(&[]);
    f.target.set_base_armor(0);
    f.character.stats_mut().increase_melee_ap(2000);
    let ap = f64::from(f.ctx().melee_ap());
    let mut time = 0.0;
    for points in [1, 5] {
        set_combo_points(&mut f, points);
        let report = cast_at(&mut f, EVISCERATE, time);
        time += 4.0;
        assert_eq!(report.combo_points_spent, points);
        assert_eq!(combo_points(&mut f), 0);
        let damage = f64::from(report.attack.unwrap().damage);
        let per_point = 170.0 + 0.03 * ap;
        let (min, max) = (54.0, 162.0);
        let points = f64::from(points);
        assert!(
            (min + per_point * points - 1.0..=max + per_point * points + 1.0).contains(&damage),
            "{damage} at {points} points with {ap} attack power"
        );
    }
    let stats = f.ctx().take_statistics();
    let finishers: Vec<_> = stats.finishers().collect();
    assert_eq!(
        finishers,
        [(&SpellKey::new("Eviscerate", 9), &[1, 0, 0, 0, 1])]
    );
}

/// A missed finisher keeps its combo points and gets 80 % of its energy back.
#[test]
fn a_missed_finisher_keeps_its_combo_points() {
    let mut f = pulled(&[]);
    set_combo_points(&mut f, 4);
    f.rig_rolls(PhysicalAttackResult::Miss);
    let report = cast_at(&mut f, EVISCERATE, 0.0);
    assert_eq!(report.result, SpellResult::Failure);
    assert_eq!(report.combo_points_spent, 0);
    assert_eq!(combo_points(&mut f), 4);
    assert_eq!(energy(&f, 0.0), 100 - 35 + 28);
}

/// Slice and Dice r2: 6 s plus 3 s per point, +30 % attack speed; Improved Slice and Dice
/// lengthens the whole duration.
#[test]
fn slice_and_dice_lasts_longer_per_combo_point() {
    let mut f = pulled(&[]);
    let speed = f.character.stats().get_melee_attack_speed_mod();
    set_combo_points(&mut f, 1);
    let report = cast_at(&mut f, SLICE_AND_DICE, 0.0);
    assert_eq!(report.combo_points_spent, 1);
    assert_eq!(duration(&mut f, SLICE_AND_DICE), Some(9.0));
    let hasted = f.character.stats().get_melee_attack_speed_mod();
    assert!((hasted / speed - 1.3).abs() < 1e-9, "{speed} -> {hasted}");
    set_combo_points(&mut f, 5);
    cast_at(&mut f, SLICE_AND_DICE, 1.0);
    assert_eq!(duration(&mut f, SLICE_AND_DICE), Some(21.0));
    // A refresh does not stack the haste, and the buff ends 21 s after it.
    assert_eq!(f.character.stats().get_melee_attack_speed_mod(), hasted);
    f.advance_to(21.9);
    assert!(f.ctx().aura_active(SLICE_AND_DICE));
    f.advance_to(22.1);
    assert!(!f.ctx().aura_active(SLICE_AND_DICE));
    assert_eq!(f.character.stats().get_melee_attack_speed_mod(), speed);

    let mut f = pulled(&IMPROVED_SLICE_AND_DICE);
    set_combo_points(&mut f, 5);
    cast_at(&mut f, SLICE_AND_DICE, 0.0);
    let duration = duration(&mut f, SLICE_AND_DICE).unwrap();
    assert!((duration - 21.0 * 1.45).abs() < 1e-9, "{duration}");
}

/// Rupture r6: a tick every 2 s for 6 s plus 2 s per point, 35 plus 4.73 per point each, plus
/// the attack power share (4 % over the ticks at 1 point, 24 % at 5) taken at the cast.
#[test]
fn rupture_ticks_by_combo_points_and_attack_power() {
    for (points, ticks, ap_percent) in [(1u32, 4u32, 4.0), (5, 8, 24.0)] {
        let mut f = pulled(&[]);
        f.character.stats_mut().increase_melee_ap(1000);
        let ap = f64::from(f.ctx().melee_ap());
        set_combo_points(&mut f, points);
        let report = cast_at(&mut f, RUPTURE, 0.0);
        assert_eq!(report.combo_points_spent, points);
        let length = 6.0 + 2.0 * f64::from(points);
        assert_eq!(duration(&mut f, RUPTURE), Some(length));
        // Attack power gained after the cast does not change the ticks.
        f.character.stats_mut().increase_melee_ap(5000);
        f.advance_to(length + 1.0);
        assert!(!f.ctx().aura_active(RUPTURE));
        let per_tick = 35.0
            + f64::from(4.73_f32) * f64::from(points)
            + ap * ap_percent / 100.0 / f64::from(ticks);
        let stats = f.ctx().take_statistics();
        let rupture = stats.spell_statistics("Rupture", 6).unwrap();
        assert_eq!(rupture.hits(), u64::from(ticks), "{points} points");
        assert_eq!(
            stats.damage_for_spell("Rupture", 6),
            u64::from(ticks) * per_tick.round() as u64,
            "{points} points"
        );
    }
}

/// Expose Armor r5: −450 armor per point for 30 s; a new cast replaces the reduction.
#[test]
fn expose_armor_reduces_armor_per_combo_point() {
    let mut f = pulled(&[]);
    let base = Mechanics::BOSS_BASE_ARMOR;
    set_combo_points(&mut f, 5);
    cast_at(&mut f, EXPOSE_ARMOR, 0.0);
    assert_eq!(f.target.armor(), base - 2250);
    set_combo_points(&mut f, 2);
    cast_at(&mut f, EXPOSE_ARMOR, 1.0);
    assert_eq!(f.target.armor(), base - 900);
    assert_eq!(duration(&mut f, EXPOSE_ARMOR), Some(30.0));
    f.advance_to(31.5);
    assert_eq!(f.target.armor(), base);
}

#[test]
fn ghostly_strike_and_hemorrhage_hit_harder_with_a_main_hand_dagger() {
    let mut f = pulled(&[]);
    assert_eq!(effect_value(&mut f, GHOSTLY_STRIKE, 0), 125.0);
    assert_eq!(effect_value(&mut f, HEMORRHAGE, 3), 100.0);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    assert_eq!(effect_value(&mut f, GHOSTLY_STRIKE, 0), 125.0);
    f.equip(EquipmentSlot::Mainhand, DAGGER);
    assert_eq!(effect_value(&mut f, GHOSTLY_STRIKE, 0), 180.0);
    assert_eq!(effect_value(&mut f, HEMORRHAGE, 3), 145.0);
}

/// Hemorrhage's debuff raises the damage of the rogue's Rupture only.
#[test]
fn hemorrhage_raises_the_rogues_rupture_damage() {
    let mut f = pulled(&[]);
    let hemorrhage = f.spell_id(HEMORRHAGE);
    f.ctx()
        .with_spell(hemorrhage, |spell, ctx| spell.enable(ctx));
    assert_eq!(damage_mod(&mut f, RUPTURE), 1.0);
    let report = cast_at(&mut f, HEMORRHAGE, 0.0);
    assert_eq!(report.result, SpellResult::Success);
    assert_eq!(combo_points(&mut f), 1);
    assert!((damage_mod(&mut f, RUPTURE) - 1.15).abs() < 1e-9);
    assert_eq!(damage_mod(&mut f, EVISCERATE), 1.0);
    f.advance_to(15.5);
    assert_eq!(damage_mod(&mut f, RUPTURE), 1.0);
}

/// Mutilate strikes with both daggers once it lands: two combo points, a main-hand and an
/// off-hand strike (with the off-hand penalty) that cannot miss on their own.
#[test]
fn mutilate_strikes_with_both_daggers() {
    let mut f = pulled(&TO_MUTILATE);
    let mutilate = highest_rank(&f, "Mutilate");
    let game_id = f.character.spells().spell(mutilate).game_id();
    assert_eq!(f.status(game_id), SpellStatus::IncorrectWeaponType, "sword");
    f.equip(EquipmentSlot::Mainhand, DAGGER);
    assert_eq!(
        f.status(game_id),
        SpellStatus::IncorrectWeaponType,
        "no off-hand dagger"
    );
    f.equip(EquipmentSlot::Offhand, DAGGER);
    f.target.set_base_armor(0);
    let ap = f64::from(f.ctx().melee_ap());

    let report = cast_at(&mut f, game_id, 0.0);
    assert_eq!(report.result, SpellResult::Success);
    assert_eq!(report.attack, None, "the strikes deal the damage");
    assert_eq!(combo_points(&mut f), 2);
    assert_eq!(report.triggered.len(), 2);
    let flat = f64::from(
        f.character
            .spells()
            .spell(f.spell_id(report.triggered[0].0))
            .record()
            .effect(0)
            .unwrap()
            .base_points,
    );
    let normalized = |weapon: f64| (weapon + ap * 1.7 / 14.0 + flat) * 0.75;
    for (index, penalty) in [(0, 1.0), (1, 0.5)] {
        let attack = report.triggered[index].1.attack.unwrap();
        let damage = f64::from(attack.damage);
        let (min, max) = (normalized(40.0) * penalty, normalized(60.0) * penalty);
        assert!(
            (min - 1.0..=max + 1.0).contains(&damage),
            "strike {index}: {damage} not in {min}..{max}"
        );
    }
    let stats = f.ctx().take_statistics();
    let rank = f.character.spells().spell(mutilate).rank();
    assert!(stats.spell_statistics("Mutilate", rank).is_some());
    assert!(stats.spell_statistics("Mutilate Off-Hand", rank).is_some());

    // A miss strikes with neither weapon and keeps the combo points at 0.
    f.rig_rolls(PhysicalAttackResult::Miss);
    let report = cast_at(&mut f, game_id, 4.0);
    assert_eq!(report.result, SpellResult::Failure);
    assert!(report.triggered.is_empty());
    assert_eq!(combo_points(&mut f), 2);
}

/// A passive's aura (Safe Fall) is not reported as a buff: it went up at the previous reset,
/// before the clock moved back to the pull, and showed a negative shortest application.
#[test]
fn passive_auras_are_not_buffs_in_the_statistics() {
    let mut f = pulled(&[]);
    assert!(f.ctx().aura_active(1860), "Safe Fall");
    f.advance_to(5.0);
    f.ctx().reset();
    let stats = f.ctx().take_statistics();
    assert!(stats.buff_statistics("Safe Fall").is_none());
}
