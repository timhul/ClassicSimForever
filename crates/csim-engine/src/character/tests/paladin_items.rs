//! The Paladin's librams and set bonuses on the shipped items (TASKS.md P.10): Libram of Fervor,
//! Economy and Law, Justice Battlegear (the Forever tier 1), Avenger's Battlegear, Battlegear of
//! Eternal Justice, Soulforge and Lightforge Armor (Crusader's Wrath at 1 PPM), Freethinker's
//! Armor and the PvP sets' spell damage.

use super::paladin::{stat, with_talents_on};
use super::paladin_seals::SEAL_OF_COMMAND_TALENT;
use super::rogue::highest_rank;
use super::rogue_items::{set_bonus_proc, shipped_equipment};
use super::*;
use crate::effect::EffectHost;
use crate::magic_school::MagicSchool;
use crate::proc::ProcSource;
use crate::target::CreatureType;

const LIBRAM_OF_FERVOR: u32 = 23203;
const SENTINELS_LIBRAM: u32 = 272434;
const LIBRAM_OF_LAW: u32 = 272435;

const JUDGEMENT: u32 = 20271;
const JUDGEMENT_OF_COMMAND: u32 = 20966;
const JUDGEMENT_OF_RIGHTEOUSNESS: u32 = 20286;
const JUDGEMENT_OF_FURY: u32 = 1311655;
const JUDGEMENT_OF_THE_CRUSADER: u32 = 20303;
const SEAL_OF_COMMAND_STRIKE: u32 = 20424;
const SWIFT_JUDGEMENT: u32 = 1310994;
const SWIFT_JUDGEMENT_TALENT: u32 = 110878;
const IMPROVED_SEAL_OF_FURY: u32 = 110875;

const CRUSADERS_WRATH_SOULFORGE: u32 = 27498;
const CRUSADERS_WRATH_LIGHTFORGE: u32 = 450625;
const ETERNAL_JUSTICE: u32 = 26135;

const AVENGERS: [(EquipmentSlot, u32); 5] = [
    (EquipmentSlot::Chest, 21389),
    (EquipmentSlot::Head, 21387),
    (EquipmentSlot::Boots, 21388),
    (EquipmentSlot::Legs, 21390),
    (EquipmentSlot::Shoulders, 21391),
];
const ETERNAL_JUSTICE_SET: [(EquipmentSlot, u32); 3] = [
    (EquipmentSlot::Back, 21397),
    (EquipmentSlot::Mainhand, 21395),
    (EquipmentSlot::Ring1, 21396),
];
const SOULFORGE: [(EquipmentSlot, u32); 6] = [
    (EquipmentSlot::Belt, 22086),
    (EquipmentSlot::Boots, 22087),
    (EquipmentSlot::Wrist, 22088),
    (EquipmentSlot::Chest, 22089),
    (EquipmentSlot::Gloves, 22090),
    (EquipmentSlot::Head, 22091),
];
const LIGHTFORGE: [(EquipmentSlot, u32); 6] = [
    (EquipmentSlot::Belt, 16723),
    (EquipmentSlot::Boots, 16725),
    (EquipmentSlot::Wrist, 16722),
    (EquipmentSlot::Chest, 16726),
    (EquipmentSlot::Gloves, 16724),
    (EquipmentSlot::Legs, 16728),
];
const FREETHINKERS: [(EquipmentSlot, u32); 2] =
    [(EquipmentSlot::Wrist, 19827), (EquipmentSlot::Belt, 19826)];
const CHAMPIONS_VINDICATION: [(EquipmentSlot, u32); 2] = [
    (EquipmentSlot::Gloves, 274227),
    (EquipmentSlot::Boots, 274226),
];

/// A Human Paladin (with the Seal of Command and `talents`) wearing `items` on the shipped
/// items, at the pull against a level 60 target, every roll rigged to a hit.
fn wearing_with(items: &[(EquipmentSlot, u32)], talents: &[(u32, u32)]) -> Fixture {
    let mut all = vec![(SEAL_OF_COMMAND_TALENT, 1)];
    all.extend_from_slice(talents);
    let mut f = with_talents_on(shipped_equipment(), &all);
    for &(slot, item) in items {
        f.equip(slot, item);
    }
    f.target.set_level(60);
    f.ctx().prepare_set_of_combat_iterations();
    f.character.set_seed(1);
    f.rig_rolls(PhysicalAttackResult::Hit);
    super::energy::pull(&mut f);
    f.advance_to(0.01);
    f
}

fn wearing(items: &[(EquipmentSlot, u32)]) -> Fixture {
    wearing_with(items, &[])
}

fn now(f: &Fixture) -> f64 {
    f.engine.current_time()
}

/// Casts the highest rank of `name` after the global cooldown of the last cast.
fn cast_named(f: &mut Fixture, name: &str) {
    let t = now(f) + 1.5;
    f.advance_to(t);
    let id = highest_rank(f, name);
    assert_eq!(f.ctx().cast(id).result, SpellResult::Success, "{name}");
}

fn judge(f: &mut Fixture) {
    let t = now(f) + 1.5;
    f.advance_to(t);
    let id = f.spell_id(JUDGEMENT);
    assert_eq!(f.ctx().cast(id).result, SpellResult::Success);
}

fn holy_damage(f: &Fixture) -> u32 {
    stat(f, |s, ctx| s.get_spell_damage(ctx, MagicSchool::Holy))
}

fn damage_mod(f: &mut Fixture, game_id: u32) -> f64 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).damage_mod(&ctx)
}

fn proc_range(f: &mut Fixture, game_id: u32, source: ProcSource) -> u32 {
    let id = set_bonus_proc(f, game_id);
    let ctx = f.ctx();
    ctx.character
        .spells()
        .procs()
        .get(id)
        .proc_range(source, &ctx)
}

fn proc_attempts(f: &Fixture, game_id: u32) -> u32 {
    let id = set_bonus_proc(f, game_id);
    f.character.spells().procs().get(id).attempts()
}

/// Libram of Fervor: Seal of the Crusader +48 attack power, Judgement of the Crusader +33 holy
/// damage taken (its `EFFECT1` and `ALL_EFFECTS` modifiers).
#[test]
fn libram_of_fervor_raises_the_crusader() {
    let crusader = |items: &[(EquipmentSlot, u32)]| {
        let mut f = wearing(items);
        let ap = f.ctx().melee_ap();
        cast_named(&mut f, "Seal of the Crusader");
        let seal_ap = f.ctx().melee_ap() - ap;
        judge(&mut f);
        (seal_ap, holy_damage(&f))
    };
    let (ap, holy) = crusader(&[]);
    assert_eq!((ap, holy), (325, 161));
    let (ap, holy) = crusader(&[(EquipmentSlot::Relic, LIBRAM_OF_FERVOR)]);
    assert_eq!((ap, holy), (325 + 48, 161 + 33));
}

/// Libram of Law: +4 % damage to the judgements its mask selects, Righteousness and Fury.
/// Not Judgement of Command (`[0, 512, 0, 0]`, the libram's second word is 8, which no spell
/// has) nor the seals' strikes.
#[test]
fn libram_of_law_raises_the_judgements() {
    let mut base = wearing(&[]);
    let mut f = wearing(&[(EquipmentSlot::Relic, LIBRAM_OF_LAW)]);
    for (judgement, factor) in [
        (JUDGEMENT_OF_RIGHTEOUSNESS, 1.04),
        (JUDGEMENT_OF_FURY, 1.04),
        (JUDGEMENT_OF_COMMAND, 1.0),
    ] {
        let ratio = damage_mod(&mut f, judgement) / damage_mod(&mut base, judgement);
        assert!((ratio - factor).abs() < 1e-9, "{judgement}: {ratio}");
    }
    assert_eq!(
        damage_mod(&mut f, SEAL_OF_COMMAND_STRIKE),
        damage_mod(&mut base, SEAL_OF_COMMAND_STRIKE)
    );
}

/// Sentinel's Libram: Swift Judgement's cooldown is 10 s shorter.
#[test]
fn sentinels_libram_shortens_swift_judgement() {
    let talents = [(IMPROVED_SEAL_OF_FURY, 1), (SWIFT_JUDGEMENT_TALENT, 1)];
    let cooldown = |items: &[(EquipmentSlot, u32)]| {
        let mut f = wearing_with(items, &talents);
        let id = f.spell_id(SWIFT_JUDGEMENT);
        let ctx = f.ctx();
        ctx.character.spells().spell(id).cooldown_seconds(&ctx)
    };
    let base = cooldown(&[]);
    assert!((cooldown(&[(EquipmentSlot::Relic, SENTINELS_LIBRAM)]) - (base - 10.0)).abs() < 1e-9);
}

/// Justice Battlegear (the Forever tier 1): its exported pieces are the "Artisan's Tier" crafted
/// ones, of which one is worn at a time, so the bonuses are learned here as the set would grant
/// them: 2 pieces +1 % attack speed, 4 pieces +36 attack power against Undead, 5 pieces
/// Judgement's cooldown 0.5 s shorter.
#[test]
fn justice_battlegear() {
    let grant = |f: &mut Fixture, bonus: u32| {
        let db = std::mem::take(&mut f.db);
        let added = f.ctx().learn(&db, bonus);
        f.db = db;
        f.ctx().enable_spell(added.spell.unwrap());
    };
    let speed = |f: &Fixture| f.character.stats().get_melee_attack_speed_mod();
    let (mut base, mut f) = (wearing(&[]), wearing(&[]));
    for bonus in [1300951, 1301084, 1301702] {
        grant(&mut f, bonus);
    }
    assert!((speed(&f) / speed(&base) - 1.01).abs() < 1e-9);
    let ap = |f: &mut Fixture| f.ctx().melee_ap();
    assert_eq!(ap(&mut f), ap(&mut base), "not against a humanoid");
    for f in [&mut base, &mut f] {
        f.target.set_creature_type(CreatureType::Undead);
    }
    assert_eq!(ap(&mut f), ap(&mut base) + 36);
    let judgement = |f: &mut Fixture| {
        let id = f.spell_id(JUDGEMENT);
        let ctx = f.ctx();
        ctx.character.spells().spell(id).cooldown_seconds(&ctx)
    };
    assert!((judgement(&mut f) - (judgement(&mut base) - 0.5)).abs() < 1e-9);
}

/// Avenger's Battlegear: 3 pieces, the judgements last 20 % longer (Judgement of the Crusader
/// 48 s); 5 pieces, +71 spell damage.
#[test]
fn avengers_battlegear() {
    let base = wearing(&[]);
    let mut f = wearing(&AVENGERS);
    assert!(holy_damage(&f) >= holy_damage(&base) + 71);
    cast_named(&mut f, "Seal of the Crusader");
    judge(&mut f);
    let buff = {
        let id = f.spell_id(JUDGEMENT_OF_THE_CRUSADER);
        f.character.spells().spell(id).marker_buff().unwrap()
    };
    let t = now(&f);
    let left = f.ctx().buff_ref(buff).time_left(t);
    assert!((left - 48.0).abs() < 1e-9, "{left}");
}

/// Battlegear of Eternal Justice 3: a landed judgement rolls the 20 % mana proc, a Holy Strike
/// or a Seal of Command strike does not.
#[test]
fn eternal_justice_procs_on_judgements() {
    let mut f = wearing(&ETERNAL_JUSTICE_SET);
    cast_named(&mut f, "Seal of Command");
    let before = proc_attempts(&f, ETERNAL_JUSTICE);
    cast_named(&mut f, "Holy Strike");
    f.ctx().trigger_spell(SEAL_OF_COMMAND_STRIKE, None);
    assert_eq!(proc_attempts(&f, ETERNAL_JUSTICE), before);
    judge(&mut f);
    assert_eq!(proc_attempts(&f, ETERNAL_JUSTICE), before + 1);
    let range = proc_range(&mut f, ETERNAL_JUSTICE, ProcSource::MainhandSpell);
    assert_eq!(range, 2000, "20 %");
}

/// Crusader's Wrath at 1 PPM (Soulforge 4 on white swings, Lightforge 5 also on spells):
/// 3.6 / 60 = 6 % a swing with the 3.6 s two-hander. Soulforge 6 gives +40 attack power,
/// Lightforge 3 +18 spell damage and 6 8 mana every 5 s.
#[test]
fn soulforge_and_lightforge() {
    let base = wearing(&[]);
    let mut f = wearing(&SOULFORGE);
    assert_eq!(
        proc_range(&mut f, CRUSADERS_WRATH_SOULFORGE, ProcSource::MainhandSwing),
        600
    );
    let ap = |f: &mut Fixture| f.ctx().melee_ap();
    let (mut base_ap, mut gear) = (base, wearing(&SOULFORGE[..5]));
    let five = ap(&mut gear) - ap(&mut base_ap);
    assert_eq!(
        ap(&mut f) - ap(&mut base_ap),
        five + 40 + stat_ap_of(&SOULFORGE[5])
    );

    let mut f = wearing(&LIGHTFORGE);
    assert_eq!(
        proc_range(
            &mut f,
            CRUSADERS_WRATH_LIGHTFORGE,
            ProcSource::MainhandSwing
        ),
        600
    );
    let mp5 = |f: &Fixture| {
        let view = f.target.stat_view();
        f.character
            .stats()
            .get_mp5(&f.character.stat_context(&view))
    };
    let base = wearing(&[]);
    assert!(holy_damage(&f) >= holy_damage(&base) + 18);
    assert!(mp5(&f) >= mp5(&base) + 8);
}

/// The attack power the item `(slot, id)` gives by its own stats (strength and attack power).
fn stat_ap_of(item: &(EquipmentSlot, u32)) -> u32 {
    let mut base = wearing(&[]);
    let mut with = wearing(&[*item]);
    with.ctx().melee_ap() - base.ctx().melee_ap()
}

/// Zandalar Freethinker's Armor 2: +4 mana every 5 s. Champion's Vindication 2: +23 spell
/// damage.
#[test]
fn freethinkers_and_pvp_bonuses() {
    let mp5 = |f: &Fixture| {
        let view = f.target.stat_view();
        f.character
            .stats()
            .get_mp5(&f.character.stat_context(&view))
    };
    let base = wearing(&[]);
    let f = wearing(&FREETHINKERS);
    assert!(mp5(&f) >= mp5(&base) + 4);
    let f = wearing(&CHAMPIONS_VINDICATION);
    assert!(holy_damage(&f) >= holy_damage(&base) + 23);
}

/// The caster buffs offered to Paladins: spell damage (Flask of Supreme Power +150, Greater
/// Arcane Elixir +35), intellect (Arcane Brilliance +31, Elixir of Greater Intellect +25,
/// Runn Tum Tuber Surprise +10), mana every 5 s (Mageblood Potion +12, Greater Blessing of
/// Wisdom 40 every 5 s, Nightfin Soup +8) and maximum mana (Flask of Distilled Wisdom +2000).
#[test]
fn caster_buffs_for_paladins() {
    let with = |names: &[&str]| {
        let mut f = wearing(&[]);
        let registry = crate::buff::external::ExternalBuffDb::load(
            &super::energy::data().join("external_buffs.yaml"),
        )
        .unwrap();
        let db = std::mem::take(&mut f.db);
        f.ctx().add_external_buffs(&registry, &db);
        f.db = db;
        for name in names {
            f.ctx().set_external_buff_selected(name, true).unwrap();
        }
        f
    };
    let intellect = |f: &Fixture| stat(f, |s, ctx| s.get_intellect(ctx));
    let mp5 = |f: &Fixture| stat(f, |s, ctx| s.get_mp5(ctx));
    let base = with(&[]);
    let f = with(&["Flask of Supreme Power", "Greater Arcane Elixir"]);
    assert_eq!(holy_damage(&f), holy_damage(&base) + 185);
    let f = with(&[
        "Arcane Brilliance",
        "Elixir of Greater Intellect",
        "Runn Tum Tuber Surprise",
    ]);
    assert_eq!(intellect(&f), intellect(&base) + 31 + 25 + 10);
    let f = with(&["Mageblood Potion", "Greater Blessing of Wisdom"]);
    assert_eq!(mp5(&f), mp5(&base) + 12 + 40);
    let f = with(&["Nightfin Soup"]);
    assert_eq!(mp5(&f), mp5(&base) + 8);
    let f = with(&["Flask of Distilled Wisdom"]);
    assert_eq!(
        f.character.max_resource_level(ResourceType::Mana),
        base.character.max_resource_level(ResourceType::Mana) + 2000
    );
}
