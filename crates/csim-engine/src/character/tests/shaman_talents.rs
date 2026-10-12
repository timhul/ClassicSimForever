//! The Shaman's talents on the shipped data (TASKS.md SH.8): both builds spend their 51 points,
//! every DPS talent works through the spell machinery, and the others change nothing. The
//! talents whose mechanics come with a spell are tested with it: Elemental Fury and Mindfulness
//! in `shaman_casting.rs`, Lava Burst and Stormstrike in `shaman_spells.rs`, Elemental Weapons
//! in `shaman_imbues.rs`, Call of Flame on the totems and Improved Fire Nova in
//! `shaman_totems.rs`.

use super::energy::pull;
use super::paladin::stat;
use super::shaman::{shaman, with_talents};
use super::*;
use crate::buff::Buff;
use crate::magic_school::MagicSchool;
use crate::spell::CastReport;

// Elemental
const CONVECTION: u32 = 104773;
const CONCUSSION: u32 = 104772;
const ELEMENTAL_WARDING: u32 = 104771;
const REVERBERATION: u32 = 104767;
const CALL_OF_FLAME: u32 = 104770;
const ELEMENTAL_DEVASTATION: u32 = 104769;
const ELEMENTAL_FOCUS: u32 = 104768;
const ELEMENTAL_ALACRITY: u32 = 104765;
const IMPROVED_FIRE_NOVA: u32 = 104764;
const EYE_OF_THE_STORM: u32 = 104763;
const CALL_OF_THUNDER: u32 = 104762;
const ELEMENTAL_REACH: u32 = 104761;
const LIGHTNING_OVERLOAD: u32 = 104759;
const ELEMENTAL_FURY: u32 = 104766;
const LAVA_BURST_TALENT: u32 = 104758;
// Enhancement
const EARTHS_GRASP: u32 = 104757;
const THUNDERING_STRIKES: u32 = 104753;
const ANCESTRAL_KNOWLEDGE: u32 = 104756;
const GUARDIAN_TOTEMS: u32 = 104754;
const MENTAL_DEXTERITY: u32 = 104755;
const IMPROVED_GHOST_WOLF: u32 = 104752;
const IMPROVED_LIGHTNING_SHIELD: u32 = 104751;
const ELEMENTAL_WEAPONS: u32 = 104750;
const SHAMANISTIC_FOCUS: u32 = 104749;
const ANTICIPATION: u32 = 104748;
const TOUGHNESS: u32 = 104746;
const FLURRY: u32 = 104747;
const STORMSTRIKE_TALENT: u32 = 104743;
const SPIRIT_WEAPONS: u32 = 104745;
const MENTAL_QUICKNESS: u32 = 104744;
const IMPROVED_STORMSTRIKE: u32 = 104742;
const MAELSTROM_WEAPON: u32 = 104741;
const RAGE_OF_THE_FARSEER_TALENT: u32 = 104740;
// Restoration
const TOTEMIC_FOCUS: u32 = 104729;
const MINDFULNESS: u32 = 104734;
const NATURAL_GRACE: u32 = 104736;
const TIDAL_FOCUS: u32 = 104735;
const NATURES_SWIFTNESS_TALENT: u32 = 104726;

const LIGHTNING_BOLT: u32 = 15208;
const LIGHTNING_BOLT_OVERLOAD: u32 = 408477;
const CHAIN_LIGHTNING: u32 = 10605;
const EARTH_SHOCK: u32 = 10414;
const FLAME_SHOCK: u32 = 29228;
const FROST_SHOCK: u32 = 10473;
const LAVA_BURST: u32 = 1238300;
const STORMSTRIKE: u32 = 17364;
const SEARING_TOTEM: u32 = 10438;
const RAGE_OF_THE_FARSEER: u32 = 425336;
const NATURES_SWIFTNESS: u32 = 16188;

const ELEMENTAL_DEVASTATION_BUFF: u32 = 30165;
const CLEARCASTING: u32 = 16246;
const FLURRY_BUFF: u32 = 16257;
const MAELSTROM_WEAPON_BUFF: u32 = 408505;
const IMPROVED_STORMSTRIKE_BUFF: u32 = 1238931;

/// The Enhancement build of TASKS.md SH.0 Q9: 8 / 31 / 12.
const ENHANCEMENT: [(u32, u32); 17] = [
    (CONCUSSION, 5),
    (ELEMENTAL_DEVASTATION, 3),
    (THUNDERING_STRIKES, 5),
    (MENTAL_DEXTERITY, 3),
    (IMPROVED_GHOST_WOLF, 2),
    (SHAMANISTIC_FOCUS, 1),
    (ELEMENTAL_WEAPONS, 3),
    (ANCESTRAL_KNOWLEDGE, 5),
    (FLURRY, 5),
    (STORMSTRIKE_TALENT, 1),
    (SPIRIT_WEAPONS, 1),
    (IMPROVED_STORMSTRIKE, 2),
    (MENTAL_QUICKNESS, 2),
    (RAGE_OF_THE_FARSEER_TALENT, 1),
    (TOTEMIC_FOCUS, 5),
    (TIDAL_FOCUS, 5),
    (MINDFULNESS, 2),
];

/// The Elemental build of TASKS.md SH.0 Q9: 32 / 6 / 13.
const ELEMENTAL: [(u32, u32); 17] = [
    (CONVECTION, 5),
    (CONCUSSION, 5),
    (ELEMENTAL_FOCUS, 1),
    (ELEMENTAL_ALACRITY, 3),
    (CALL_OF_FLAME, 1),
    (CALL_OF_THUNDER, 1),
    (EYE_OF_THE_STORM, 3),
    (IMPROVED_FIRE_NOVA, 2),
    (LIGHTNING_OVERLOAD, 3),
    (ELEMENTAL_REACH, 2),
    (ELEMENTAL_FURY, 5),
    (LAVA_BURST_TALENT, 1),
    (ANCESTRAL_KNOWLEDGE, 1),
    (THUNDERING_STRIKES, 5),
    (TOTEMIC_FOCUS, 5),
    (MINDFULNESS, 3),
    (TIDAL_FOCUS, 5),
];

/// An Orc Shaman with `talents` forced in against a level 60 target without armor, seeded,
/// pulled.
fn pulled(talents: &[(u32, u32)]) -> Fixture {
    let mut f = with_talents(talents);
    f.target.set_level(60);
    f.target.set_base_armor(0);
    f.character.set_seed(1);
    pull(&mut f);
    f.advance_to(0.01);
    f
}

fn now(f: &Fixture) -> f64 {
    f.engine.current_time()
}

/// Lets `seconds` pass.
fn wait(f: &mut Fixture, seconds: f64) {
    let t = now(f) + seconds;
    f.advance_to(t);
}

/// Casts `game_id` as soon as it is castable and lets its cast time pass.
fn cast(f: &mut Fixture, game_id: u32) -> CastReport {
    while f.status(game_id) != SpellStatus::Available {
        wait(f, 0.1);
    }
    let seconds = cast_time(f, game_id);
    let id = f.spell_id(game_id);
    let report = f.ctx().cast(id);
    if seconds > 0.0 {
        wait(f, seconds + 0.001);
    }
    report
}

fn cost(f: &mut Fixture, game_id: u32) -> u32 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).resource_cost(&ctx)
}

fn cast_time(f: &mut Fixture, game_id: u32) -> f64 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).cast_time(&ctx)
}

fn cooldown_left(f: &mut Fixture, game_id: u32) -> f64 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).cooldown_remaining(&ctx)
}

fn damage_mod(f: &mut Fixture, game_id: u32) -> f64 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).damage_mod(&ctx)
}

fn crit_bonus(f: &mut Fixture, game_id: u32) -> u32 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).crit_chance_bonus(&ctx)
}

/// The shaman's buff of spell `game_id`, when it is up.
fn buff(f: &Fixture, game_id: u32) -> Option<&Buff> {
    let spells = f.character.spells();
    spells
        .buff_ids()
        .filter_map(|id| spells.owned_buff(id))
        .find(|buff| buff.spell() == game_id && buff.is_active())
}

fn mana(f: &Fixture) -> u32 {
    f.character.resource_level(ResourceType::Mana, 0.0)
}

fn refill_mana(f: &mut Fixture) {
    let t = now(f);
    f.character.gain_resource(ResourceType::Mana, 100_000, t);
}

fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} vs {expected}");
}

#[test]
fn both_builds_spend_51_points() {
    for (build, tabs) in [(&ENHANCEMENT, [8, 31, 12]), (&ELEMENTAL, [32, 6, 13])] {
        let f = shaman(Race::Orc, build);
        let talents = f.character.talents().unwrap();
        assert_eq!(talents.spent_points(), 51);
        assert_eq!(
            [
                talents.tab_points(375),
                talents.tab_points(373),
                talents.tab_points(374)
            ],
            tabs
        );
    }
}

/// Convection 5/5: -10 % mana of the shocks, Lightning Bolt, Chain Lightning and Lava Burst;
/// Shamanistic Focus: -45 % of the shocks; Totemic Focus 5/5: -25 % of the totems.
#[test]
fn convection_shamanistic_and_totemic_focus_reduce_costs() {
    let mut base = pulled(&[(LIGHTNING_OVERLOAD, 3), (LAVA_BURST_TALENT, 1)]);
    let mut f = pulled(&[
        (LIGHTNING_OVERLOAD, 3),
        (LAVA_BURST_TALENT, 1),
        (CONVECTION, 5),
        (SHAMANISTIC_FOCUS, 1),
        (TOTEMIC_FOCUS, 5),
    ]);
    for (game_id, percent) in [
        (LIGHTNING_BOLT, 90.0),
        (CHAIN_LIGHTNING, 90.0),
        (LAVA_BURST, 90.0),
        (EARTH_SHOCK, 45.0),
        (FLAME_SHOCK, 45.0),
        (FROST_SHOCK, 45.0),
        (SEARING_TOTEM, 75.0),
        (STORMSTRIKE, 100.0),
    ] {
        let expected = f64::from(cost(&mut base, game_id)) * percent / 100.0;
        let actual = f64::from(cost(&mut f, game_id));
        assert!(
            (actual - expected).abs() <= 1.0,
            "{game_id}: {actual} vs {expected}"
        );
    }
}

/// Concussion 5/5: Lightning Bolt, Chain Lightning and Earth Shock deal 5 % more.
#[test]
fn concussion_raises_lightning_and_earth_shock() {
    let mut f = pulled(&[(CONCUSSION, 5)]);
    for game_id in [LIGHTNING_BOLT, CHAIN_LIGHTNING, EARTH_SHOCK] {
        assert_close(damage_mod(&mut f, game_id), 1.05);
    }
    for game_id in [FLAME_SHOCK, FROST_SHOCK] {
        assert_close(damage_mod(&mut f, game_id), 1.0);
    }
}

/// Reverberation 5/5: the shocks' shared cooldown is 5 s.
#[test]
fn reverberation_shortens_the_shocks() {
    let mut f = pulled(&[(REVERBERATION, 5)]);
    cast(&mut f, EARTH_SHOCK);
    for game_id in [EARTH_SHOCK, FLAME_SHOCK, FROST_SHOCK] {
        assert!(
            (cooldown_left(&mut f, game_id) - 5.0).abs() < 0.01,
            "{game_id}"
        );
    }
}

/// Call of Flame 3/3: Flame Shock and Lava Burst deal 15 % more, Flame Shock's ticks 15 % more
/// again (the second effect: periodic damage).
#[test]
fn call_of_flame_raises_the_fire_spells() {
    let mut f = pulled(&[
        (CALL_OF_FLAME, 3),
        (LIGHTNING_OVERLOAD, 3),
        (LAVA_BURST_TALENT, 1),
    ]);
    assert_close(damage_mod(&mut f, FLAME_SHOCK), 1.15);
    assert_close(damage_mod(&mut f, LAVA_BURST), 1.15);
    assert_close(damage_mod(&mut f, LIGHTNING_BOLT), 1.0);
    let id = f.spell_id(FLAME_SHOCK);
    let ctx = f.ctx();
    let periodic = ctx.character.spells().spell(id).periodic_damage_mod(&ctx);
    assert_close(periodic, 1.15 * 1.15);
}

/// Elemental Devastation 3/3: a spell crit gives +9 % melee crit for 10 s.
#[test]
fn elemental_devastation_follows_a_spell_crit() {
    let mut f = pulled(&[(ELEMENTAL_DEVASTATION, 3)]);
    f.rig_rolls(PhysicalAttackResult::Hit);
    let melee = |f: &Fixture| stat(f, |s, ctx| s.get_mh_crit_chance(ctx));
    let before = melee(&f);
    cast(&mut f, EARTH_SHOCK);
    assert!(buff(&f, ELEMENTAL_DEVASTATION_BUFF).is_none(), "a hit");

    f.character.stats_mut().increase_spell_crit(10_000);
    cast(&mut f, EARTH_SHOCK);
    assert!(buff(&f, ELEMENTAL_DEVASTATION_BUFF).is_some(), "a crit");
    let after = melee(&f);
    f.character.stats_mut().decrease_melee_aura_crit(900);
    assert_eq!(melee(&f), before, "{after}: 9 % aura crit");
    f.character.stats_mut().increase_melee_aura_crit(900);
    wait(&mut f, 10.0);
    assert!(buff(&f, ELEMENTAL_DEVASTATION_BUFF).is_none(), "10 s");
}

/// Elemental Focus: 10 % of the damage spells give Clearcasting, which makes the next damage
/// spell free.
#[test]
fn elemental_focus_gives_clearcasting() {
    let mut f = pulled(&[(ELEMENTAL_FOCUS, 1)]);
    let mut free = 0;
    for _ in 0..300 {
        refill_mana(&mut f);
        let before = mana(&f);
        let clear = buff(&f, CLEARCASTING).is_some();
        if clear {
            free += 1;
            assert_eq!(cost(&mut f, LIGHTNING_BOLT), 0);
            assert_eq!(cost(&mut f, EARTH_SHOCK), 0);
            assert_ne!(cost(&mut f, STORMSTRIKE), 0, "a physical strike");
        }
        cast(&mut f, LIGHTNING_BOLT);
        let spent = before - mana(&f);
        assert_eq!(spent == 0, clear, "{spent}");
    }
    assert!((15..=45).contains(&free), "{free} of 300");
}

/// Elemental Alacrity 3/3: Lightning Bolt, Chain Lightning and Lava Burst 0.5 s faster.
#[test]
fn elemental_alacrity_shortens_the_casts() {
    let mut f = pulled(&[
        (ELEMENTAL_ALACRITY, 3),
        (LIGHTNING_OVERLOAD, 3),
        (LAVA_BURST_TALENT, 1),
    ]);
    assert_close(cast_time(&mut f, LIGHTNING_BOLT), 2.0);
    assert_close(cast_time(&mut f, CHAIN_LIGHTNING), 1.5);
    assert_close(cast_time(&mut f, LAVA_BURST), 2.0);
}

/// Call of Thunder: +3 % crit for Lightning Bolt and Chain Lightning.
#[test]
fn call_of_thunder_raises_lightning_crit() {
    let mut f = pulled(&[(ELEMENTAL_ALACRITY, 3), (CALL_OF_THUNDER, 1)]);
    assert_eq!(crit_bonus(&mut f, LIGHTNING_BOLT), 300);
    assert_eq!(crit_bonus(&mut f, CHAIN_LIGHTNING), 300);
    assert_eq!(crit_bonus(&mut f, EARTH_SHOCK), 0);
}

/// Casts Lightning Bolt `times` times with all the mana it needs; the overloads land with the
/// casts.
fn bolts(f: &mut Fixture, times: u32) {
    for _ in 0..times {
        refill_mana(f);
        cast(f, LIGHTNING_BOLT);
    }
}

fn attempts(f: &Fixture, name: &str) -> u64 {
    f.character
        .statistics
        .spells()
        .filter(|(key, _)| key.name == name)
        .map(|(_, spell)| spell.total_attempts())
        .sum()
}

/// Lightning Overload 3/3: 10 % of the Lightning Bolts that land cast the overload copy for
/// half the damage at no cost; the copies neither overload again nor count for Elemental Focus.
#[test]
fn lightning_overload_casts_the_copy() {
    let mut f = pulled(&[(LIGHTNING_OVERLOAD, 3), (ELEMENTAL_FOCUS, 1)]);
    bolts(&mut f, 400);
    let casts = attempts(&f, "Lightning Bolt");
    let copies = attempts(&f, "Lightning Bolt Overload");
    assert_eq!(casts, 400);
    assert!((20..=60).contains(&copies), "{copies}");
    let focus = f
        .character
        .spells()
        .procs()
        .find_by_game_id(16164)
        .map(|id| f.character.spells().procs().get(id).attempts())
        .unwrap();
    assert!(u64::from(focus) <= casts, "only the casts: {focus}");

    let none = pulled(&[(LIGHTNING_OVERLOAD, 0), (ELEMENTAL_FOCUS, 1)]);
    let _ = none;
    assert_eq!(cost(&mut f, LIGHTNING_BOLT_OVERLOAD), 0);
    assert_close(cast_time(&mut f, LIGHTNING_BOLT_OVERLOAD), 0.0);
}

/// The first Lightning Bolt or Lava Burst that overloads with 1000 spell damage, every roll a
/// hit and `prepare` done before the cast; its damage and its copy's.
fn first_overload(game_id: u32, prepare: impl Fn(&mut Fixture)) -> (u32, u32) {
    for seed in 0..200 {
        let mut f = pulled(&[
            (LIGHTNING_OVERLOAD, 3),
            (LAVA_BURST_TALENT, 1),
            (STORMSTRIKE_TALENT, 1),
        ]);
        f.character.set_seed(seed);
        f.rig_rolls(PhysicalAttackResult::Hit);
        f.character.stats_mut().increase_base_spell_damage(1000);
        prepare(&mut f);
        let report = cast(&mut f, game_id);
        let _ = report;
        let ends: Vec<(String, u64)> = f
            .character
            .statistics
            .spells()
            .filter(|(_, spell)| spell.total_damage() > 0)
            .map(|(key, spell)| (key.name.clone(), spell.total_damage()))
            .collect();
        let name = f
            .character
            .spells()
            .spell(f.spell_id(game_id))
            .name()
            .to_owned();
        let damage = |suffix: &str| {
            ends.iter()
                .find(|(n, _)| *n == format!("{name}{suffix}"))
                .map(|(_, d)| *d as u32)
        };
        if let (Some(cast), Some(copy)) = (damage(""), damage(" Overload")) {
            return (cast, copy);
        }
    }
    panic!("no overload in 200 seeds");
}

/// The copy of a Stormstrike-empowered Lightning Bolt gets no bonus: the cast used the
/// charge. A Lava Burst's copy keeps its Flame Shock bonus.
#[test]
fn the_copy_after_stormstrike_and_flame_shock() {
    // Lightning Bolt at 60: 200.8 ± 5.4 % + 0.714 × 1000; its copy 100.4 ± 5.7 % + 0.357 × 1000.
    let (cast, copy) = first_overload(LIGHTNING_BOLT, |f| {
        f.ctx().trigger_spell(STORMSTRIKE, None);
    });
    assert!(f64::from(cast) >= (190.0 + 714.0) * 1.2, "{cast}");
    assert!(f64::from(copy) <= 107.0 + 357.0, "{copy}");

    // Lava Burst: 220 ± 12.7 % + 714; its copy 110 ± 12.9 % + 357, both × 1.2.
    let (cast, copy) = first_overload(LAVA_BURST, |f| {
        f.ctx().trigger_spell(FLAME_SHOCK, None);
    });
    assert!(f64::from(cast) >= (192.0 + 714.0) * 1.2, "{cast}");
    assert!(f64::from(copy) >= (95.0 + 357.0) * 1.2, "{copy}");
}

/// Thundering Strikes 5/5: +5 % crit with spells and attacks.
#[test]
fn thundering_strikes_raises_crit() {
    let melee = |f: &Fixture| stat(f, |s, ctx| s.get_mh_crit_chance(ctx));
    let spell = |f: &Fixture| {
        stat(f, |s, ctx| {
            s.get_spell_crit_chance(ctx, MagicSchool::Nature)
        })
    };
    let base = pulled(&[]);
    let f = pulled(&[(THUNDERING_STRIKES, 5)]);
    assert_eq!(spell(&f), spell(&base) + 500);
    assert!(melee(&f) > melee(&base));
}

/// Ancestral Knowledge 5/5: +10 % intellect; Toughness 5/5: +10 % stamina.
#[test]
fn ancestral_knowledge_and_toughness_raise_attributes() {
    let intellect = |f: &Fixture| stat(f, |s, ctx| s.get_intellect(ctx));
    let stamina = |f: &Fixture| stat(f, |s, ctx| s.get_stamina(ctx));
    let base = pulled(&[]);
    let f = pulled(&[(ANCESTRAL_KNOWLEDGE, 5), (TOUGHNESS, 5)]);
    assert_eq!(
        intellect(&f),
        (f64::from(intellect(&base)) * 1.1).round() as u32
    );
    assert_eq!(
        stamina(&f),
        (f64::from(stamina(&base)) * 1.1).round() as u32
    );
}

/// Mental Dexterity 3/3: attack power equal to the intellect; Mental Quickness 2/2: spell
/// damage of 30 % of it.
#[test]
fn mental_dexterity_and_quickness_scale_with_intellect() {
    let ap = |f: &Fixture| stat(f, |s, ctx| s.get_melee_ap(ctx));
    let damage = |f: &Fixture| stat(f, |s, ctx| s.get_spell_damage(ctx, MagicSchool::Nature));
    let intellect = |f: &Fixture| stat(f, |s, ctx| s.get_intellect(ctx));
    let base = pulled(&[]);
    let f = pulled(&[(MENTAL_DEXTERITY, 3), (MENTAL_QUICKNESS, 2)]);
    assert_eq!(ap(&f), ap(&base) + intellect(&f));
    assert_eq!(damage(&f), damage(&base) + intellect(&f) * 30 / 100);

    let mut f = pulled(&[(MENTAL_DEXTERITY, 1)]);
    let before = ap(&f);
    f.character.stats_mut().increase_intellect(100);
    assert_eq!(ap(&f), before + 33, "33 % of 100 more intellect");
}

/// Flurry 5/5: a melee crit gives +25 % attack speed for three swings.
#[test]
fn flurry_follows_a_melee_crit() {
    let mut f = pulled(&[(MENTAL_DEXTERITY, 3), (FLURRY, 5), (STORMSTRIKE_TALENT, 1)]);
    let speed = |f: &Fixture| stat(f, |s, _| s.get_melee_attack_speed_mod());
    let before = speed(&f);
    f.character.stats_mut().increase_melee_aura_crit(10_000);
    loop {
        let report = cast(&mut f, STORMSTRIKE);
        if report
            .attack
            .is_some_and(|a| a.result == PhysicalAttackResult::Critical)
        {
            break;
        }
        assert!(buff(&f, FLURRY_BUFF).is_none(), "no crit, no Flurry");
    }
    let flurry = buff(&f, FLURRY_BUFF).expect("Flurry");
    assert_eq!(flurry.charges(), 3);
    assert_close(speed(&f), before * 1.25);
}

/// Improved Stormstrike 2/2: Stormstrike gives 50 % mana regeneration while casting for 15 s.
#[test]
fn improved_stormstrike_follows_stormstrike() {
    let mut f = pulled(&[(STORMSTRIKE_TALENT, 1), (IMPROVED_STORMSTRIKE, 2)]);
    f.rig_rolls(PhysicalAttackResult::Hit);
    cast(&mut f, STORMSTRIKE);
    assert!(buff(&f, IMPROVED_STORMSTRIKE_BUFF).is_some());
    wait(&mut f, 15.0);
    assert!(buff(&f, IMPROVED_STORMSTRIKE_BUFF).is_none());
}

/// Maelstrom Weapon 5/5: melee hits stack it up to 5; each stack takes 20 % off the next
/// Lightning Bolt's cast time and mana, and that bolt uses every stack.
#[test]
fn maelstrom_weapon_stacks_and_speeds_lightning_bolt() {
    let mut f = pulled(&[(MAELSTROM_WEAPON, 5)]);
    f.rig_rolls(PhysicalAttackResult::Hit);
    let (time, mana_cost) = (
        cast_time(&mut f, LIGHTNING_BOLT),
        cost(&mut f, LIGHTNING_BOLT),
    );
    f.ctx().start_attack();
    let mut stacks = 0;
    while stacks < 5 {
        assert!(now(&f) < 120.0, "5 stacks in 2 minutes");
        wait(&mut f, 0.1);
        let current = buff(&f, MAELSTROM_WEAPON_BUFF).map_or(0, Buff::stacks);
        if current != stacks {
            stacks = current;
            let left = 1.0 - 0.2 * f64::from(stacks);
            assert_close(cast_time(&mut f, LIGHTNING_BOLT), time * left);
            let expected = f64::from(mana_cost) * left;
            let actual = f64::from(cost(&mut f, LIGHTNING_BOLT));
            assert!((actual - expected).abs() <= 1.0, "{actual} vs {expected}");
        }
    }
    assert_eq!(cast_time(&mut f, LIGHTNING_BOLT), 0.0);
    let id = f.spell_id(LIGHTNING_BOLT);
    while f.status(LIGHTNING_BOLT) != SpellStatus::Available {
        wait(&mut f, 0.1);
    }
    assert!(!f.ctx().cast(id).cast_started, "instant: the swings go on");
    assert!(
        buff(&f, MAELSTROM_WEAPON_BUFF).is_none_or(|b| b.stacks() < 5),
        "the bolt used the stacks"
    );
}

/// Rage of the Farseer: +30 % attack speed for 25 s, a 3 minute cooldown, off the global
/// cooldown.
#[test]
fn rage_of_the_farseer_hastes_the_swings() {
    let mut f = pulled(&[(MENTAL_QUICKNESS, 2), (RAGE_OF_THE_FARSEER_TALENT, 1)]);
    let speed = |f: &Fixture| stat(f, |s, _| s.get_melee_attack_speed_mod());
    let before = speed(&f);
    cast(&mut f, RAGE_OF_THE_FARSEER);
    assert_eq!(f.status(EARTH_SHOCK), SpellStatus::Available);
    assert_close(speed(&f), before * 1.3);
    assert!((cooldown_left(&mut f, RAGE_OF_THE_FARSEER) - 180.0).abs() < 0.01);
    wait(&mut f, 25.0);
    assert_close(speed(&f), before);
}

/// Tidal Focus 5/5: +5 % melee and spell hit.
#[test]
fn tidal_focus_raises_hit() {
    let melee = |f: &Fixture| stat(f, |s, ctx| s.get_melee_hit_chance(ctx));
    let spell = |f: &Fixture| stat(f, |s, ctx| s.get_spell_hit_chance(ctx, MagicSchool::Nature));
    let base = pulled(&[]);
    let f = pulled(&[(TIDAL_FOCUS, 5)]);
    assert_eq!(melee(&f), melee(&base) + 500);
    assert_eq!(spell(&f), spell(&base) + 500);
}

/// Nature's Swiftness: the next nature spell is instant, and only that one.
#[test]
fn natures_swiftness_makes_the_next_nature_spell_instant() {
    let mut f = pulled(&[(NATURES_SWIFTNESS_TALENT, 1)]);
    cast(&mut f, NATURES_SWIFTNESS);
    assert_eq!(cast_time(&mut f, LIGHTNING_BOLT), 0.0);
    cast(&mut f, LIGHTNING_BOLT);
    assert_close(cast_time(&mut f, LIGHTNING_BOLT), 2.5);
}

/// The talents that only defend, move or heal change none of the damage stats.
#[test]
fn defensive_and_utility_talents_change_nothing() {
    let summary = |f: &Fixture| {
        stat(f, |s, ctx| {
            [
                s.get_melee_ap(ctx),
                s.get_mh_crit_chance(ctx),
                s.get_melee_hit_chance(ctx),
                s.get_spell_damage(ctx, MagicSchool::Nature),
                s.get_spell_crit_chance(ctx, MagicSchool::Nature),
                s.get_spell_hit_chance(ctx, MagicSchool::Nature),
                s.get_strength(ctx),
                s.get_agility(ctx),
                s.get_intellect(ctx),
            ]
        })
    };
    let mut base = pulled(&[]);
    let mut f = pulled(&[
        (ELEMENTAL_WARDING, 3),
        (EYE_OF_THE_STORM, 3),
        (ELEMENTAL_REACH, 2),
        (EARTHS_GRASP, 2),
        (GUARDIAN_TOTEMS, 2),
        (IMPROVED_GHOST_WOLF, 2),
        (IMPROVED_LIGHTNING_SHIELD, 3),
        (ANTICIPATION, 3),
        (SPIRIT_WEAPONS, 1),
        (NATURAL_GRACE, 3),
    ]);
    assert_eq!(summary(&f), summary(&base));
    for game_id in [LIGHTNING_BOLT, EARTH_SHOCK, FLAME_SHOCK] {
        assert_eq!(cost(&mut f, game_id), cost(&mut base, game_id), "{game_id}");
        assert_eq!(damage_mod(&mut f, game_id), damage_mod(&mut base, game_id));
    }
}
