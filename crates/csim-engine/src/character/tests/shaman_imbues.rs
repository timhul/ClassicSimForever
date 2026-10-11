//! The Shaman's weapon imbues on the shipped data (TASKS.md SH.6): Windfury, Flametongue and
//! Rockbiter Weapon as temporary enchants whose `SpellItemEnchantment` equip spell is a passive
//! of the weapon, their exclusions, and Elemental Weapons on each.

use super::energy::pull;
use super::shaman::with_talents;
use super::*;
use crate::character_spells::{EquipmentGrantor, SpellHandle};
use crate::effect::EffectHost;
use crate::equipment::EnchantError;
use crate::ids::ProcId;

const WINDFURY_ATTACK: u32 = 16361;
const FLAMETONGUE_ATTACK: &str = "Flametongue Attack";

const ELEMENTAL_WEAPONS: u32 = 104750;

/// An Orc Shaman with `talents` forced in, a 3.6 s two-handed axe imbued with `imbue`, against
/// a level 60 target without armor, pulled.
fn imbued(talents: &[(u32, u32)], imbue: EnchantName) -> Fixture {
    let mut f = with_talents(talents);
    f.target.set_level(60);
    f.target.set_base_armor(0);
    f.equip(EquipmentSlot::Mainhand, TWO_HAND_AXE);
    f.set_temp_enchants(EquipmentSlot::Mainhand, &[imbue]);
    f.ctx().prepare_set_of_combat_iterations();
    f.ctx().reset();
    f.character.set_seed(1);
    pull(&mut f);
    f.advance_to(0.01);
    f
}

/// The weapon's passive the imbue grants, as a proc.
fn imbue_proc(f: &Fixture) -> ProcId {
    f.character
        .spells()
        .equipment_spells()
        .find_map(|(key, handle)| match (key.grantor, handle) {
            (EquipmentGrantor::Enchant(_), SpellHandle::Proc(id)) => Some(id),
            _ => None,
        })
        .expect("the imbue's proc")
}

fn melee_ap(f: &mut Fixture) -> u32 {
    f.ctx().melee_ap()
}

/// One imbue a weapon: it takes the weapon's temporary enchant like a stone or an oil, and
/// Windfury Weapon replaces the shaman's own Windfury Totem. A stone goes with Windfury Totem.
#[test]
fn an_imbue_takes_the_temporary_enchant() {
    let mut f = with_talents(&[]);
    f.equip(EquipmentSlot::Mainhand, TWO_HAND_AXE);
    let db = std::mem::take(&mut f.db);
    let set = |f: &mut Fixture, enchants: &[EnchantName]| {
        f.ctx()
            .set_temp_enchants(&db, EquipmentSlot::Mainhand, enchants)
    };
    for (pair, ok) in [
        (
            [EnchantName::WindfuryWeapon, EnchantName::FlametongueWeapon],
            false,
        ),
        (
            [EnchantName::RockbiterWeapon, EnchantName::FlametongueWeapon],
            false,
        ),
        (
            [EnchantName::WindfuryWeapon, EnchantName::WindfuryTotem],
            false,
        ),
        (
            [EnchantName::WindfuryTotem, EnchantName::WindfuryWeapon],
            false,
        ),
        (
            [
                EnchantName::FlametongueWeapon,
                EnchantName::ElementalSharpeningStone,
            ],
            false,
        ),
        (
            [
                EnchantName::BrilliantWizardOil,
                EnchantName::RockbiterWeapon,
            ],
            false,
        ),
        (
            [EnchantName::FlametongueWeapon, EnchantName::WindfuryTotem],
            true,
        ),
        (
            [EnchantName::RockbiterWeapon, EnchantName::WindfuryTotem],
            true,
        ),
        (
            [
                EnchantName::WindfuryTotem,
                EnchantName::ElementalSharpeningStone,
            ],
            true,
        ),
    ] {
        let result = set(&mut f, &pair);
        assert_eq!(result.is_ok(), ok, "{pair:?}: {result:?}");
        if !ok {
            assert!(matches!(result, Err(EnchantError::SameGroup { .. })));
        }
    }
    // Adding one replaces what it cannot be with.
    set(
        &mut f,
        &[
            EnchantName::WindfuryTotem,
            EnchantName::ElementalSharpeningStone,
        ],
    )
    .unwrap();
    f.ctx()
        .add_temp_enchant(&db, EquipmentSlot::Mainhand, EnchantName::WindfuryWeapon)
        .unwrap();
    assert_eq!(
        f.character
            .equipment()
            .temp_enchants(EquipmentSlot::Mainhand),
        [EnchantName::WindfuryWeapon]
    );
    f.db = db;
}

/// Rockbiter Weapon: +554 attack power plus 16.5 per level above 54 (653 at 60) while the
/// weapon is worn; Elemental Weapons 3/3 adds 20 % of it.
#[test]
fn rockbiter_adds_attack_power() {
    let mut plain = imbued(&[], EnchantName::WindfuryTotem);
    let base = melee_ap(&mut plain);
    let mut rock = imbued(&[], EnchantName::RockbiterWeapon);
    assert_eq!(melee_ap(&mut rock) - base, 653);
    let mut rock = imbued(&[(ELEMENTAL_WEAPONS, 3)], EnchantName::RockbiterWeapon);
    let mut talented = imbued(&[(ELEMENTAL_WEAPONS, 3)], EnchantName::WindfuryTotem);
    let base = melee_ap(&mut talented);
    assert_eq!(melee_ap(&mut rock) - base, 784, "653 x 1.2");
}

/// Flametongue Weapon: every landed melee hit adds rank 6's 2498 + 78 per level above 56 (2810
/// at 60) x the weapon speed / 100 fire: 101 with a 3.6 s two-hander, plus 0.1 of spell power
/// (Flametongue Attack's coefficient); Elemental Weapons 3/3: 15 % more.
#[test]
fn flametongue_burns_by_weapon_speed() {
    let per_hit = |talents: &[(u32, u32)], power: u32| {
        let mut f = imbued(talents, EnchantName::FlametongueWeapon);
        f.character.stats_mut().increase_base_spell_damage(power);
        f.rig_rolls(PhysicalAttackResult::Hit);
        let id = imbue_proc(&f);
        let report = f.ctx().perform_proc(id);
        let (_, attack) = report
            .triggered
            .first()
            .expect("the proc casts Flametongue Attack");
        let attack = attack.attack.expect("it deals damage");
        assert_eq!(attack.result, PhysicalAttackResult::Hit);
        attack.damage
    };
    assert_eq!(per_hit(&[], 0), 101, "2810 x 3.6 / 100");
    assert_eq!(per_hit(&[], 1000), 201, "0.1 x 1000 spell power");
    assert_eq!(per_hit(&[(ELEMENTAL_WEAPONS, 3)], 0), 116, "101.16 x 1.15");

    // It procs on the hits of the fight.
    let mut f = imbued(&[], EnchantName::FlametongueWeapon);
    f.ctx().start_attack();
    f.advance_to(60.0);
    let stats = &f.character.statistics;
    let swings = stats
        .spells()
        .filter(|(key, _)| key.name == "Mainhand Attack")
        .map(|(_, s)| s.hits_including_partial_resists() + s.crits() + s.glances())
        .sum::<u64>();
    let fire = stats
        .spells()
        .filter(|(key, _)| key.name == FLAMETONGUE_ATTACK)
        .map(|(_, s)| s.total_attempts())
        .sum::<u64>();
    assert!(swings > 10, "{swings}");
    assert_eq!(fire, swings, "one per landed swing");
}

/// Windfury Weapon: its passive procs at 20 % with a 1.5 s internal cooldown on melee hits and
/// casts 16361: two extra attacks and +333 attack power (Elemental Weapons 3/3: +40 %, 466) for
/// 1.5 s and three swings.
#[test]
fn windfury_weapon() {
    let mut f = imbued(&[], EnchantName::WindfuryWeapon);
    let id = imbue_proc(&f);
    {
        let ctx = f.ctx();
        let proc = ctx.character.spells().procs().get(id);
        assert_eq!(proc.spell().record().aura_options.proc_chance, 20);
        assert_eq!(
            proc.spell().record().aura_options.proc_category_recovery_ms,
            1500
        );
    }
    for (talents, bonus) in [(&[][..], 333), (&[(ELEMENTAL_WEAPONS, 3)][..], 466)] {
        let mut f = imbued(talents, EnchantName::WindfuryWeapon);
        let base = melee_ap(&mut f);
        f.ctx().trigger_spell(WINDFURY_ATTACK, None).unwrap();
        assert_eq!(melee_ap(&mut f) - base, bonus, "{talents:?}");
    }

    // A long fight: about a fifth of the swings that may proc it do, each one followed by two
    // extra attacks, which never roll it (nor does a swing within 1.5 s of a proc).
    let mut f = imbued(&[], EnchantName::WindfuryWeapon);
    let id = imbue_proc(&f);
    f.ctx().start_attack();
    f.advance_to(1800.0);
    let proc = f.character.spells().procs().get(id);
    let (attempts, procs) = (f64::from(proc.attempts()), f64::from(proc.procs()));
    let rate = procs / attempts;
    assert!(
        (0.17..=0.23).contains(&rate),
        "{procs} of {attempts}: {rate}"
    );
    let swings = swings_of(&f.character.statistics);
    let normal = (1800.0_f64 / 3.6).floor();
    assert!(
        (swings - (normal + 2.0 * procs)).abs() <= 2.0,
        "{swings} swings, {procs} procs"
    );
    assert!(
        attempts <= normal + 1.0,
        "{attempts} rolls of {normal} swings"
    );
}

/// Every main-hand swing of `stats`, landed or not.
fn swings_of(stats: &crate::statistics::ClassStatistics) -> f64 {
    stats
        .spells()
        .filter(|(key, _)| key.name == "Mainhand Attack")
        .map(|(_, s)| s.total_attempts())
        .sum::<u64>() as f64
}
