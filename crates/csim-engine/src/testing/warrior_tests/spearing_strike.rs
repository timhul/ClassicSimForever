//! Spearing Strike, the Forever-only Arms talent (spell 1310222); no C++ counterpart. It deals
//! 40 % of the normalized weapon damage (`WEAPON_PERCENT_DAMAGE` 40 scaling
//! `NORMALIZED_WEAPON_DMG` 0), needs a two-handed weapon (`SpellEquippedItems`: two-hand axe,
//! mace, sword, polearm or staff), costs 15 rage and has a
//! 20 s cooldown. Against giants and dragonkin it deals an additional 2 × 40 % (the DUMMY
//! effect, `EXTRA_WEAPON_DAMAGE_VS_CREATURE_TYPES`); the sim has no mounted targets.

use crate::engine::EventType;
use crate::spell::SpellStatus;
use crate::target::CreatureType;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Spearing Strike";

fn test() -> WarriorTest {
    WarriorTest::new(SPELL)
}

fn test_with_twohander() -> WarriorTest {
    let mut test = test();
    test.given_a_twohand_weapon_with_100_min_max_dmg();
    test
}

#[test]
fn name_correct() {
    let mut test = test();
    let id = test.spell(SPELL);
    assert_eq!(test.character().spells().spell(id).name(), SPELL);
}

#[test]
fn spell_cooldown() {
    let mut test = test_with_twohander();
    test.given_a_guaranteed_melee_ability_hit();
    assert_eq!(test.base_cooldown(SPELL), "20.000");

    test.cast(SPELL);

    test.then_next_event_is(EventType::PlayerAction, "1.500", false);
    test.then_next_event_is(EventType::PlayerAction, "20.000", false);
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test_with_twohander();
    assert!(test.action_ready());
    test.cast(SPELL);
    assert!(!test.action_ready());
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test_with_twohander();
    test.enable_spell(SPELL);
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_is_on_gcd();

    test.then_status_is(SPELL, SpellStatus::OnGcd);
    assert_eq!(test.cooldown_remaining(SPELL), 0.0);
}

#[test]
fn is_ready_conditions() {
    let mut test = test_with_twohander();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::NotEnabled);

    test.enable_spell(SPELL);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn needs_a_twohanded_weapon() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.enable_spell(SPELL);
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::IncorrectWeaponType);
}

#[test]
fn usable_in_every_stance() {
    let mut test = test_with_twohander();
    test.enable_spell(SPELL);
    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    let mut test = test_with_twohander();
    test.enable_spell(SPELL);
    test.given_warrior_in_defensive_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn resource_cost() {
    let mut test = test_with_twohander();
    test.given_a_guaranteed_melee_ability_hit();
    test.given_warrior_has_rage(15);
    test.cast(SPELL);
    test.then_warrior_has_rage(0);
}

#[test]
fn insufficient_rage() {
    let mut test = test_with_twohander();
    test.enable_spell(SPELL);
    test.given_warrior_has_rage(14);
    test.then_status_is(SPELL, SpellStatus::InsufficientResources);
}

/// Spearing Strike with the 100 - 100 two-hander against an unarmored `creature` target,
/// 1000 AP.
fn damage_vs(creature: CreatureType, crit: bool, impale: u32) -> u64 {
    let mut test = test();
    test.given_target_has_0_armor();
    test.target_mut().set_creature_type(creature);
    test.given_a_twohand_weapon_with_100_min_max_dmg();
    if crit {
        test.given_a_guaranteed_melee_ability_crit();
    } else {
        test.given_a_guaranteed_melee_ability_hit();
    }
    test.given_1000_melee_ap();
    test.given_no_previous_damage_dealt();
    test.given_impale(impale);
    test.cast(SPELL);
    test.damage_dealt()
}

/// [`damage_vs`] a humanoid, which gets no bonus.
fn damage(crit: bool, impale: u32) -> u64 {
    damage_vs(CreatureType::Humanoid, crit, impale)
}

#[test]
fn hit_dmg() {
    // The percentage scales the normalized weapon damage, it is not added to it:
    // [Damage] = (base_dmg + normalized_wpn_speed * AP / 14) * 40 %
    // [134] = (100 + 3.3 * 1000 / 14) * 0.4
    assert_eq!(damage(false, 0), 134);
}

#[test]
fn crit_dmg_0_of_2_impale() {
    // [269] = (100 + 3.3 * 1000 / 14) * 0.4 * 2.0
    assert_eq!(damage(true, 0), 269);
}

#[test]
fn crit_dmg_2_of_2_impale() {
    // [295] = (100 + 3.3 * 1000 / 14) * 0.4 * 2.2
    assert_eq!(damage(true, 2), 295);
}

#[test]
fn hit_dmg_vs_giants_and_dragonkin() {
    // 40 % plus the additional 2 × 40 %:
    // [403] = (100 + 3.3 * 1000 / 14) * 0.4 * (1 + 2)
    assert_eq!(damage_vs(CreatureType::Giant, false, 0), 403);
    assert_eq!(damage_vs(CreatureType::Dragonkin, false, 0), 403);
}

#[test]
fn crit_dmg_vs_giants() {
    // [886] = (100 + 3.3 * 1000 / 14) * 0.4 * 3 * 2.2
    assert_eq!(damage_vs(CreatureType::Giant, true, 2), 886);
}

#[test]
fn no_bonus_vs_other_creature_types() {
    for creature in CreatureType::ALL {
        if !matches!(creature, CreatureType::Giant | CreatureType::Dragonkin) {
            assert_eq!(damage_vs(creature, false, 0), 134, "{creature:?}");
        }
    }
}

#[test]
fn dodge_applies_overpower_buff() {
    let mut test = test_with_twohander();
    test.given_a_guaranteed_melee_ability_dodge();
    test.cast(SPELL);
    test.then_overpower_is_active();
}
