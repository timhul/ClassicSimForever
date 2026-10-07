//! The Paladin on the shipped data: the class file's base stats per race, the base and
//! intellect crit, and the maximum mana from intellect.

use super::energy::data;
use super::*;
use crate::magic_school::MagicSchool;
use crate::resource::Mana;
use crate::talent::{CharacterTalents, TalentDb};

/// A Paladin of `race` with the shipped spells and tree, `talents` spent, a two-handed axe in
/// the main hand, ready for an iteration.
pub(super) fn paladin(race_kind: Race, talents: &[(u32, u32)]) -> Fixture {
    let classes = crate::character::ClassDb::load(&data().join("classes"), None).unwrap();
    let class = Arc::clone(classes.get(crate::faction::PlayerClass::Paladin).unwrap());
    let mut f = Fixture::of_race(class, race_kind, equipment_db_with_enchants());
    f.db = SpellDb::load(&data().join("spells")).expect("shipped spell data loads");
    let trees = TalentDb::load(&data().join("talents")).unwrap();
    let tree = Arc::clone(trees.get(crate::faction::PlayerClass::Paladin).unwrap());
    f.ctx().set_talents(CharacterTalents::new(tree));
    f.equip(EquipmentSlot::Mainhand, TWO_HAND_AXE);
    let db = std::mem::take(&mut f.db);
    f.ctx().learn_all(&db);
    f.db = db;
    assert_eq!(f.ctx().spend_talent_points(talents), []);
    f.ctx().prepare_set_of_combat_iterations();
    f.ctx().reset();
    f
}

/// A Human Paladin with `talents` forced in (the tiers below them need no points), ready for
/// an iteration.
pub(super) fn with_talents(talents: &[(u32, u32)]) -> Fixture {
    let mut f = paladin(Race::Human, &[]);
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
    f.ctx().reset();
    f
}

pub(super) fn stat<R>(
    f: &Fixture,
    read: impl FnOnce(&crate::stats::CharacterStats, &crate::stats::StatContext) -> R,
) -> R {
    let view = f.target.stat_view();
    read(f.character.stats(), &f.character.stat_context(&view))
}

fn mana(f: &Fixture) -> (u32, u32) {
    (
        f.character.resource_level(ResourceType::Mana, 0.0),
        f.character.max_resource_level(ResourceType::Mana),
    )
}

/// Class (85 / 45 / 80 / 50 / 55) plus race, before any spell is learned.
#[test]
fn base_stats_combine_class_and_race() {
    let classes = crate::character::ClassDb::load(&data().join("classes"), None).unwrap();
    let class = classes.get(crate::faction::PlayerClass::Paladin).unwrap();
    for (race_kind, expected) in [
        (Race::Human, [105, 65, 100, 70, 77]),
        (Race::Dwarf, [107, 61, 103, 69, 74]),
        (Race::Undead, [104, 63, 101, 68, 80]),
    ] {
        let f = Fixture::of_race(Arc::clone(class), race_kind, equipment_db());
        let attributes = stat(&f, |s, ctx| {
            [
                s.get_strength(ctx),
                s.get_agility(ctx),
                s.get_stamina(ctx),
                s.get_intellect(ctx),
                s.get_spirit(ctx),
            ]
        });
        assert_eq!(attributes, expected, "{race_kind:?}");
        assert_eq!(
            stat(&f, |s, ctx| s.get_melee_ap(ctx)),
            160 + 2 * expected[0],
            "{race_kind:?}: 2 attack power per strength"
        );
    }
}

/// 3.336 % base spell crit (334) plus 1 % per 59.88 intellect, for every school.
#[test]
fn spell_crit_from_base_and_intellect() {
    let f = paladin(Race::Undead, &[]);
    let intellect = stat(&f, |s, ctx| s.get_intellect(ctx));
    let from_intellect = (f64::from(intellect) / 59.8802 * 100.0).round() as u32;
    for school in [MagicSchool::Holy, MagicSchool::Fire] {
        assert_eq!(
            stat(&f, |s, ctx| s.get_spell_crit_chance(ctx, school)),
            334 + from_intellect,
            "{school:?}"
        );
    }
}

/// The maximum is the base 1512 plus 1 mana for each of the first 20 intellect and 15 for
/// each point beyond, and an iteration starts full.
#[test]
fn max_mana_from_intellect() {
    for race_kind in [Race::Human, Race::Dwarf, Race::Undead] {
        let f = paladin(race_kind, &[]);
        let intellect = stat(&f, |s, ctx| s.get_intellect(ctx));
        let max = 1512 + Mana::mana_from_intellect(intellect);
        assert_eq!(mana(&f), (max, max), "{race_kind:?}");
    }
    // Undead: 50 + 18 intellect = 20 + 48 x 15 = 740 mana from intellect.
    let f = paladin(Race::Undead, &[]);
    assert_eq!(mana(&f), (2252, 2252));
}

/// The two-hander, shield and libram the class file allows; no daggers.
#[test]
fn proficiencies() {
    let f = paladin(Race::Human, &[]);
    let class = f.character.class();
    assert!(class.can_wield(EquipmentSlot::Mainhand, WeaponType::TwohandAxe));
    assert!(class.can_wield(EquipmentSlot::Offhand, WeaponType::Shield));
    assert!(class.can_wield(EquipmentSlot::Ranged, WeaponType::Libram));
    assert!(!class.can_wield(EquipmentSlot::Mainhand, WeaponType::Dagger));
    assert!(!class.can_wield(EquipmentSlot::Offhand, WeaponType::Sword));
    assert!(class.can_wear(crate::item::ArmorType::Plate));
}
