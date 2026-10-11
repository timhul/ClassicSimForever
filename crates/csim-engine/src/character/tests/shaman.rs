//! The Shaman on the shipped data: the class file's base stats per race, the base and
//! intellect crit, the maximum mana from intellect, the proficiencies and the baseline passives.

use super::energy::data;
use super::paladin::stat;
use super::*;
use crate::magic_school::MagicSchool;
use crate::resource::Mana;
use crate::talent::{CharacterTalents, TalentDb};

const ENHANCING_TOTEMS: u32 = 16295;
const IMPROVED_WEAPON_TOTEMS: u32 = 29193;
const WEAPON_MASTERY: u32 = 29088;
const HEALING_GRACE: u32 = 29191;

const RACES: [Race; 5] = [
    Race::Orc,
    Race::Dwarf,
    Race::Tauren,
    Race::Troll,
    Race::WindshaperSkyborne,
];

/// A Shaman of `race` with the shipped spells and tree, `talents` spent, a one-handed mace in
/// the main hand (no racial weapon crit: the Orc's Axe Specialization), ready for an iteration.
pub(super) fn shaman(race_kind: Race, talents: &[(u32, u32)]) -> Fixture {
    let classes = crate::character::ClassDb::load(&data().join("classes"), None).unwrap();
    let class = Arc::clone(classes.get(crate::faction::PlayerClass::Shaman).unwrap());
    let mut f = Fixture::of_race(class, race_kind, equipment_db_with_enchants());
    f.db = SpellDb::load(&data().join("spells")).expect("shipped spell data loads");
    let trees = TalentDb::load(&data().join("talents")).unwrap();
    let tree = Arc::clone(trees.get(crate::faction::PlayerClass::Shaman).unwrap());
    f.ctx().set_talents(CharacterTalents::new(tree));
    f.equip(EquipmentSlot::Mainhand, MACE);
    let db = std::mem::take(&mut f.db);
    f.ctx().learn_all(&db);
    f.db = db;
    assert_eq!(f.ctx().spend_talent_points(talents), []);
    f.ctx().prepare_set_of_combat_iterations();
    f.ctx().reset();
    f
}

fn mana(f: &Fixture) -> (u32, u32) {
    (
        f.character.resource_level(ResourceType::Mana, 0.0),
        f.character.max_resource_level(ResourceType::Mana),
    )
}

/// Class (65 / 35 / 75 / 70 / 80) plus race, before any spell is learned.
#[test]
fn base_stats_combine_class_and_race() {
    let classes = crate::character::ClassDb::load(&data().join("classes"), None).unwrap();
    let class = classes.get(crate::faction::PlayerClass::Shaman).unwrap();
    for (race_kind, expected) in [
        (Race::Orc, [88, 52, 97, 87, 103]),
        (Race::Dwarf, [87, 51, 98, 89, 99]),
        (Race::Tauren, [90, 50, 97, 85, 102]),
        (Race::Troll, [86, 57, 96, 86, 101]),
        (Race::WindshaperSkyborne, [84, 56, 94, 91, 100]),
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
            "{race_kind:?}: 2 attack power per strength, none per agility"
        );
    }
}

/// 1.7 % base melee crit plus 1 % per 19.685 agility; 3.54 % base spell crit plus 1 % per
/// 59.17 intellect, for every school.
#[test]
fn crit_from_base_and_stats() {
    let f = shaman(Race::Orc, &[]);
    let agility = stat(&f, |s, ctx| s.get_agility(ctx));
    let from_agility = (f64::from(agility) / 19.685 * 100.0).round() as u32;
    let melee = stat(&f, |s, ctx| s.get_mh_crit_chance(ctx));
    assert!(
        melee.abs_diff(170 + from_agility) <= 1,
        "{melee} vs {}",
        170 + from_agility
    );
    let intellect = stat(&f, |s, ctx| s.get_intellect(ctx));
    let from_intellect = (f64::from(intellect) / 59.1716 * 100.0).round() as u32;
    for school in [MagicSchool::Nature, MagicSchool::Fire, MagicSchool::Frost] {
        assert_eq!(
            stat(&f, |s, ctx| s.get_spell_crit_chance(ctx, school)),
            354 + from_intellect,
            "{school:?}"
        );
    }
}

/// The maximum is the base 1520 plus 1 mana for each of the first 20 intellect and 15 for
/// each point beyond, and an iteration starts full.
#[test]
fn max_mana_from_intellect() {
    for race_kind in RACES {
        let f = shaman(race_kind, &[]);
        let intellect = stat(&f, |s, ctx| s.get_intellect(ctx));
        let max = 1520 + Mana::mana_from_intellect(intellect);
        assert_eq!(mana(&f), (max, max), "{race_kind:?}");
    }
    // Orc: 70 + 17 intellect = 20 + 67 x 15 = 1025 mana from intellect.
    let f = shaman(Race::Orc, &[]);
    assert_eq!(mana(&f), (2545, 2545));
}

/// 17 + spirit / 5 mana per 5 seconds.
#[test]
fn spirit_regeneration() {
    let f = shaman(Race::Orc, &[]);
    let regen = f.character.class().mana_regen.unwrap();
    assert_eq!(regen.mp5_from_spirit(103), 17.0 + 103.0 / 5.0);
}

/// Two-handed and one-handed axes and maces, shields, caster off-hands and totems; no swords,
/// polearms or off-hand weapons (no Dual Wield).
#[test]
fn proficiencies() {
    let f = shaman(Race::Orc, &[]);
    let class = f.character.class();
    for weapon in [
        WeaponType::TwohandAxe,
        WeaponType::TwohandMace,
        WeaponType::Axe,
        WeaponType::Mace,
        WeaponType::Dagger,
        WeaponType::Fist,
        WeaponType::Staff,
    ] {
        assert!(
            class.can_wield(EquipmentSlot::Mainhand, weapon),
            "{weapon:?}"
        );
    }
    assert!(class.can_wield(EquipmentSlot::Offhand, WeaponType::Shield));
    assert!(class.can_wield(EquipmentSlot::Offhand, WeaponType::CasterOffhand));
    assert!(class.can_wield(EquipmentSlot::Relic, WeaponType::Totem));
    for weapon in [
        WeaponType::Sword,
        WeaponType::TwohandSword,
        WeaponType::Polearm,
    ] {
        assert!(
            !class.can_wield(EquipmentSlot::Mainhand, weapon),
            "{weapon:?}"
        );
    }
    for weapon in [WeaponType::Axe, WeaponType::Mace, WeaponType::Dagger] {
        assert!(
            !class.can_wield(EquipmentSlot::Offhand, weapon),
            "{weapon:?}"
        );
    }
    assert!(class.can_wear(crate::item::ArmorType::Mail));
    assert!(!class.can_wear(crate::item::ArmorType::Plate));
}

/// Enhancing Totems and Improved Weapon Totems rank 2 are baseline; Weapon Mastery and
/// Healing Grace, 1.12 talents left in the skill lines, never apply.
#[test]
fn baseline_passives() {
    let f = shaman(Race::Orc, &[]);
    let spells = &f.character.spells;
    let spell = |game_id| {
        spells.spell(
            spells
                .spell_by_game_id(game_id)
                .unwrap_or_else(|| panic!("{game_id} learned")),
        )
    };
    for game_id in [ENHANCING_TOTEMS, IMPROVED_WEAPON_TOTEMS] {
        assert!(spell(game_id).is_enabled(), "{game_id}");
    }
    for game_id in [WEAPON_MASTERY, HEALING_GRACE] {
        assert!(spell(game_id).is_ignored(), "{game_id}");
    }
}
