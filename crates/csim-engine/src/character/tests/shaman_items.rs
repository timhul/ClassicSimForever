//! The Shaman's totems, set bonuses and caster buffs on the shipped items (TASKS.md SH.9): the
//! two Totems of the Storm (Lightning Bolt's spell damage, Lightning Bolt into Maelstrom
//! Weapon), Burning Totem, The Elements, Augur's Regalia, Stormcaller's Garb, Champion's
//! Stormcaller, The Spiritcaller's Storm (the Forever tier 1), Curse of the Elements and the
//! caster consumables.

use super::energy::pull;
use super::paladin::stat;
use super::rogue_items::{set_bonus_proc, shipped_equipment};
use super::shaman::with_talents_on;
use super::*;
use crate::buff::Buff;
use crate::buff::external::ExternalBuffDb;
use crate::effect::EffectHost;
use crate::faction::{Faction, PlayerClass};
use crate::magic_school::MagicSchool;
use crate::proc::{ProcHost, ProcSource};
use crate::spell::CastReport;
use crate::target::CreatureType;

const TOTEM_OF_THE_STORM: u32 = 23199;
const FOREVER_TOTEM_OF_THE_STORM: u32 = 272432;
const BURNING_TOTEM: u32 = 272433;

const LIGHTNING_BOLT: u32 = 15208;
const CHAIN_LIGHTNING: u32 = 10605;
const EARTH_SHOCK: u32 = 10414;
const FLAME_SHOCK: u32 = 29228;
const LAVA_BURST: u32 = 1238300;

const LIGHTNING_OVERLOAD: u32 = 104759;
const LAVA_BURST_TALENT: u32 = 104758;
const MAELSTROM_WEAPON: u32 = 104741;

const MAELSTROM_WEAPON_PROC: u32 = 408498;
const MAELSTROM_WEAPON_BUFF: u32 = 408505;
const STORMCALLERS_WRATH: u32 = 26121;
const STORMCALLER_BONUS: u32 = 26119;
const THE_FURIOUS_STORM: u32 = 450626;

const ELEMENTS: [(EquipmentSlot, u32); 5] = [
    (EquipmentSlot::Belt, 16673),
    (EquipmentSlot::Boots, 16670),
    (EquipmentSlot::Wrist, 16671),
    (EquipmentSlot::Head, 16667),
    (EquipmentSlot::Gloves, 16672),
];
const AUGURS: [(EquipmentSlot, u32); 2] =
    [(EquipmentSlot::Wrist, 19830), (EquipmentSlot::Belt, 19829)];
const STORMCALLERS: [(EquipmentSlot, u32); 3] = [
    (EquipmentSlot::Head, 21372),
    (EquipmentSlot::Boots, 21373),
    (EquipmentSlot::Chest, 21374),
];
const CHAMPIONS_STORMCALLER: [(EquipmentSlot, u32); 4] = [
    (EquipmentSlot::Boots, 22857),
    (EquipmentSlot::Gloves, 22867),
    (EquipmentSlot::Chest, 22876),
    (EquipmentSlot::Legs, 22887),
];

/// An Orc Shaman with `talents` wearing `items` on the shipped items, at the pull against a
/// level 60 target without armor, seeded, every roll rigged to a hit unless `rolls`.
fn wearing_with(items: &[(EquipmentSlot, u32)], talents: &[(u32, u32)], rolls: bool) -> Fixture {
    let mut f = with_talents_on(shipped_equipment(), talents);
    for &(slot, item) in items {
        f.equip(slot, item);
    }
    f.target.set_level(60);
    f.target.set_base_armor(0);
    f.ctx().prepare_set_of_combat_iterations();
    f.ctx().reset();
    f.character.set_seed(1);
    if !rolls {
        f.rig_rolls(PhysicalAttackResult::Hit);
    }
    pull(&mut f);
    f.advance_to(0.01);
    f
}

fn wearing(items: &[(EquipmentSlot, u32)]) -> Fixture {
    wearing_with(items, &[], false)
}

fn now(f: &Fixture) -> f64 {
    f.engine.current_time()
}

fn wait(f: &mut Fixture, seconds: f64) {
    let t = now(f) + seconds;
    f.advance_to(t);
}

/// Casts `game_id` as soon as it is castable, with mana to spare, and lets its cast time pass.
fn cast(f: &mut Fixture, game_id: u32) -> CastReport {
    while f.status(game_id) != SpellStatus::Available {
        wait(f, 0.1);
    }
    let t = now(f);
    f.character.gain_resource(ResourceType::Mana, 100_000, t);
    let id = f.spell_id(game_id);
    let seconds = {
        let ctx = f.ctx();
        ctx.character.spells().spell(id).cast_time(&ctx)
    };
    let report = f.ctx().cast(id);
    if seconds > 0.0 {
        wait(f, seconds + 0.001);
    }
    report
}

/// The damage of one triggered `game_id`.
fn damage(f: &mut Fixture, game_id: u32) -> u32 {
    f.ctx()
        .trigger_spell(game_id, None)
        .and_then(|report| report.attack)
        .expect("the spell dealt damage")
        .damage
}

fn buff(f: &Fixture, game_id: u32) -> Option<&Buff> {
    let spells = f.character.spells();
    spells
        .buff_ids()
        .filter_map(|id| spells.owned_buff(id))
        .find(|buff| buff.spell() == game_id && buff.is_active())
}

fn nature_damage(f: &Fixture) -> u32 {
    stat(f, |s, ctx| s.get_spell_damage(ctx, MagicSchool::Nature))
}

/// A Shaman's relics are its totems: no libram, idol or ranged weapon fits the relic slot.
#[test]
fn a_shaman_wears_totems() {
    let mut f = wearing(&[]);
    for totem in [
        TOTEM_OF_THE_STORM,
        FOREVER_TOTEM_OF_THE_STORM,
        BURNING_TOTEM,
    ] {
        f.equip(EquipmentSlot::Relic, totem);
    }
    let class = f.character.class();
    assert!(class.can_wield(EquipmentSlot::Relic, WeaponType::Totem));
    assert!(!class.can_wield(EquipmentSlot::Relic, WeaponType::Libram));
}

/// Totem of the Storm (23199): Lightning Bolt and Chain Lightning +33 spell damage, through
/// their coefficients (0.714 and 0.571); not Earth Shock.
#[test]
fn totem_of_the_storm_adds_lightning_damage() {
    let relic = [(EquipmentSlot::Relic, TOTEM_OF_THE_STORM)];
    for (game_id, coefficient) in [
        (LIGHTNING_BOLT, 0.714),
        (CHAIN_LIGHTNING, 0.571),
        (EARTH_SHOCK, 0.0),
    ] {
        let base = damage(&mut wearing(&[]), game_id);
        let with = damage(&mut wearing(&relic), game_id);
        let gained = f64::from(with) - f64::from(base);
        assert!(
            (gained - 33.0 * coefficient).abs() <= 1.0,
            "{game_id}: {gained}"
        );
    }
    assert_eq!(
        nature_damage(&wearing(&relic)),
        nature_damage(&wearing(&[]))
    );
}

/// Maelstrom Weapon procs from melee hits only; Totem of the Storm (272432) lets Lightning
/// Bolt trigger it at half the chance (5 PPM by the weapon's speed), and only Lightning Bolt.
#[test]
fn totem_of_the_storm_lets_lightning_bolt_proc_maelstrom() {
    let proc_chance = |f: &mut Fixture, game_id: u32| {
        let id = f
            .character
            .spells()
            .procs()
            .find_by_game_id(MAELSTROM_WEAPON_PROC)
            .unwrap();
        let class = f
            .character
            .spells()
            .spell(f.spell_id(game_id))
            .record()
            .class_options;
        let ctx = f.ctx();
        let speed = ctx.base_weapon_speed(Hand::Mainhand).unwrap();
        let factor = ctx.proc_from_spell_factor(MAELSTROM_WEAPON_PROC, class.as_ref());
        let range = ctx
            .character
            .spells()
            .procs()
            .get(id)
            .proc_range(ProcSource::MainhandSwing, &ctx);
        (speed, factor, range)
    };
    let talents = [(MAELSTROM_WEAPON, 5)];
    let mut without = wearing_with(&[], &talents, true);
    for _ in 0..30 {
        cast(&mut without, LIGHTNING_BOLT);
    }
    assert!(
        buff(&without, MAELSTROM_WEAPON_BUFF).is_none(),
        "no spell procs it"
    );
    let (speed, factor, range) = proc_chance(&mut without, LIGHTNING_BOLT);
    assert_eq!(factor, None);
    assert_eq!(range, (10.0 * speed / 60.0 * 10_000.0_f64).round() as u32);

    let relic = [(EquipmentSlot::Relic, FOREVER_TOTEM_OF_THE_STORM)];
    let mut with = wearing_with(&relic, &talents, true);
    assert_eq!(proc_chance(&mut with, LIGHTNING_BOLT).1, Some(0.5));
    assert_eq!(proc_chance(&mut with, EARTH_SHOCK).1, None);
    let mut stacked = false;
    for _ in 0..30 {
        cast(&mut with, LIGHTNING_BOLT);
        stacked |= buff(&with, MAELSTROM_WEAPON_BUFF).is_some();
    }
    assert!(stacked, "Lightning Bolt procs it");
}

/// Burning Totem: Flame Shock lasts 3 s longer.
#[test]
fn burning_totem_lengthens_flame_shock() {
    let ticks = |items: &[(EquipmentSlot, u32)]| {
        let mut f = wearing(items);
        cast(&mut f, FLAME_SHOCK);
        wait(&mut f, 20.0);
        f.character
            .statistics
            .spells()
            .filter(|(key, _)| key.name == "Flame Shock")
            .map(|(_, spell)| spell.total_damage())
            .sum::<u64>()
    };
    let base = ticks(&[]);
    let longer = ticks(&[(EquipmentSlot::Relic, BURNING_TOTEM)]);
    assert!(longer > base, "{longer} vs {base}");
}

/// The Elements: 3 pieces +18 spell damage (beyond what the pieces carry); 5 pieces The
/// Furious Storm on swings and spell casts at 1 PPM.
#[test]
fn the_elements() {
    let base = nature_damage(&wearing(&[]));
    let two = nature_damage(&wearing(&ELEMENTS[..2]));
    let third = nature_damage(&wearing(&ELEMENTS[2..3])) - base;
    assert_eq!(nature_damage(&wearing(&ELEMENTS[..3])), two + third + 18);

    let mut f = wearing(&ELEMENTS);
    let id = set_bonus_proc(&f, THE_FURIOUS_STORM);
    let ctx = f.ctx();
    let speed = ctx.base_weapon_speed(Hand::Mainhand).unwrap();
    let range = ctx
        .character
        .spells()
        .procs()
        .get(id)
        .proc_range(ProcSource::MainhandSwing, &ctx);
    assert_eq!(range, (speed / 60.0 * 10_000.0_f64).round() as u32);
}

/// Augur's Regalia: 2 pieces 4 mana per 5 s.
#[test]
fn augurs_regalia_regenerates_mana() {
    let mp5 = |f: &Fixture| stat(f, |s, ctx| s.get_mp5(ctx));
    let one = wearing(&AUGURS[..1]);
    let both = wearing(&AUGURS);
    let single = wearing(&AUGURS[1..]);
    let base = wearing(&[]);
    let items = (mp5(&one) - mp5(&base)) + (mp5(&single) - mp5(&base));
    assert_eq!(mp5(&both), mp5(&base) + items + 4);
}

/// Stormcaller's Garb 3: Lightning Bolt, Chain Lightning and the shocks have a 20 % chance to
/// give Stormcaller's Wrath (+50 nature damage, 8 s); Lava Burst does not.
#[test]
fn stormcallers_garb_gives_stormcallers_wrath() {
    let talents = [(LIGHTNING_OVERLOAD, 3), (LAVA_BURST_TALENT, 1)];
    let mut f = wearing_with(&STORMCALLERS, &talents, false);
    let id = set_bonus_proc(&f, STORMCALLER_BONUS);
    for _ in 0..20 {
        cast(&mut f, LAVA_BURST);
    }
    assert_eq!(f.character.spells().procs().get(id).attempts(), 0);
    let mut wrath = false;
    for _ in 0..20 {
        let before = nature_damage(&f);
        cast(&mut f, LIGHTNING_BOLT);
        if buff(&f, STORMCALLERS_WRATH).is_some() {
            wrath = true;
            assert_eq!(nature_damage(&f), before + 50);
            break;
        }
    }
    assert!(wrath);
    assert!(f.character.spells().procs().get(id).attempts() > 0);
    wait(&mut f, 8.0);
    assert!(buff(&f, STORMCALLERS_WRATH).is_none(), "8 s");
}

/// Champion's Stormcaller: 2 pieces +40 attack power, 4 pieces +2 % shock crit.
#[test]
fn champions_stormcaller() {
    let ap = |f: &mut Fixture| f.ctx().melee_ap();
    let crit = |f: &mut Fixture, game_id: u32| {
        let id = f.spell_id(game_id);
        let ctx = f.ctx();
        ctx.character.spells().spell(id).crit_chance_bonus(&ctx)
    };
    let mut two = wearing(&CHAMPIONS_STORMCALLER[..2]);
    let mut first = wearing(&CHAMPIONS_STORMCALLER[..1]);
    let mut second = wearing(&CHAMPIONS_STORMCALLER[1..2]);
    let mut base = wearing(&[]);
    let items = ap(&mut first) + ap(&mut second) - 2 * ap(&mut base);
    assert_eq!(ap(&mut two), ap(&mut base) + items + 40);
    let mut four = wearing(&CHAMPIONS_STORMCALLER);
    assert_eq!(crit(&mut four, EARTH_SHOCK), 200);
    assert_eq!(crit(&mut four, FLAME_SHOCK), 200);
    assert_eq!(crit(&mut four, LIGHTNING_BOLT), 0);
}

/// The Spiritcaller's Storm (the Forever tier 1): its raid pieces are not exported yet, so the
/// bonuses are learned here as the set would grant them: 2 pieces +1 % melee and spell hit,
/// 4 pieces +21 spell damage against Elementals, 5 pieces Lava Burst's cooldown 1 s shorter.
#[test]
fn the_spiritcallers_storm() {
    let grant = |f: &mut Fixture, bonus: u32| {
        let db = std::mem::take(&mut f.db);
        let added = f.ctx().learn(&db, bonus);
        f.db = db;
        f.ctx().enable_spell(added.spell.unwrap());
    };
    let talents = [(LIGHTNING_OVERLOAD, 3), (LAVA_BURST_TALENT, 1)];
    let mut f = wearing_with(&[], &talents, false);
    let hit = |f: &Fixture| {
        stat(f, |s, ctx| {
            (
                s.get_melee_hit_chance(ctx),
                s.get_spell_hit_chance(ctx, MagicSchool::Nature),
            )
        })
    };
    let before = hit(&f);
    grant(&mut f, 1300959);
    assert_eq!(hit(&f), (before.0 + 100, before.1 + 100));

    let before = nature_damage(&f);
    grant(&mut f, 1301092);
    assert_eq!(nature_damage(&f), before, "a boss is no Elemental");
    f.target.set_creature_type(CreatureType::Elemental);
    assert_eq!(nature_damage(&f), before + 21);

    grant(&mut f, 1301711);
    cast(&mut f, LAVA_BURST);
    let id = f.spell_id(LAVA_BURST);
    let ctx = f.ctx();
    let left = ctx.character.spells().spell(id).cooldown_remaining(&ctx);
    assert!(
        (left - (9.0 - 2.5)).abs() < 0.01,
        "9 s from the cast's start: {left}"
    );
}

/// Curse of the Elements (Forever's): every magic school takes 10 % more and loses 75
/// resistance; offered to the casters only.
#[test]
fn curse_of_the_elements() {
    let registry = ExternalBuffDb::load(&data_dir().join("external_buffs.yaml")).unwrap();
    let curse = registry
        .debuffs()
        .iter()
        .find(|d| d.name == "Curse of the Elements")
        .unwrap();
    assert_eq!(curse.classes, [PlayerClass::Paladin, PlayerClass::Shaman]);
    // Two Shamans seeded the same: the same damage roll.
    let base = damage(&mut wearing(&[]), LIGHTNING_BOLT);
    let mut f = wearing(&[]);
    let db = std::mem::take(&mut f.db);
    f.ctx().add_external_buffs(&registry, &db);
    f.db = db;
    f.ctx()
        .toggle_external_buff("Curse of the Elements")
        .unwrap();
    let cursed = damage(&mut f, LIGHTNING_BOLT);
    assert!(
        (f64::from(cursed) - f64::from(base) * 1.1).abs() <= 1.0,
        "{cursed} vs {base}"
    );
}

/// The caster raid buffs, flasks, elixirs, food and mana consumables are offered to a Shaman,
/// Elixir of Greater Firepower too (fire: Flame Shock, Lava Burst, the totems).
#[test]
fn caster_buffs_are_offered_to_a_shaman() {
    let registry = ExternalBuffDb::load(&data_dir().join("external_buffs.yaml")).unwrap();
    let mut f = wearing(&[]);
    let db = std::mem::take(&mut f.db);
    f.ctx().add_external_buffs(&registry, &db);
    f.db = db;
    let offered: Vec<String> = f
        .character
        .external_buffs()
        .offered(Faction::Horde)
        .map(|e| e.spec.name.clone())
        .collect();
    for name in [
        "Arcane Brilliance",
        "Greater Blessing of Wisdom",
        "Flask of Supreme Power",
        "Flask of Distilled Wisdom",
        "Greater Arcane Elixir",
        "Elixir of Greater Firepower",
        "Mageblood Potion",
        "Nightfin Soup",
        "Battle Shout",
    ] {
        assert!(offered.iter().any(|o| o == name), "{name}");
    }
    let fire = stat(&f, |s, ctx| s.get_spell_damage(ctx, MagicSchool::Fire));
    f.ctx()
        .toggle_external_buff("Elixir of Greater Firepower")
        .unwrap();
    assert_eq!(
        stat(&f, |s, ctx| s.get_spell_damage(ctx, MagicSchool::Fire)),
        fire + 40
    );
}

fn data_dir() -> std::path::PathBuf {
    super::energy::data()
}
