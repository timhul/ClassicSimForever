//! Missiles (`SpellMisc.Speed`): a cast rolls, spends and uses its spell modifier charges at
//! launch and deals its damage, procs and other charges when it lands, the distance over the
//! speed later. A caster rotation stands 30 yd away, a melee one 5 yd.

use super::energy::pull;
use super::*;
use crate::attack_mode::AttackMode;
use crate::buff::Buff;
use crate::spell::SpellHost;
use crate::target::CreatureType;

const LIGHTNING_BOLT: u32 = 15208;
const CHAIN_LIGHTNING: u32 = 10605;
const EARTH_SHOCK: u32 = 10414;
const FLAME_SHOCK: u32 = 29228;
const LAVA_BURST: u32 = 1238300;
const CLEARCASTING: u32 = 16246;
const ELEMENTAL_FOCUS_PROC: u32 = 16164;
const HOLY_WRATH: u32 = 10318;
const SEARING_TOTEM: u32 = 10438;

const ELEMENTAL_FOCUS: u32 = 104768;
const LAVA_BURST_TALENT: u32 = 104758;
const LIGHTNING_OVERLOAD: u32 = 104759;

/// A Shaman with `talents` on a caster's rotation (30 yd), pulled, against an armorless level
/// 60 target.
fn caster(talents: &[(u32, u32)]) -> Fixture {
    let mut f = super::shaman::with_talents(talents);
    f.character.set_sim(SimParams::default());
    f.character.spells.set_attack_mode(AttackMode::MagicAttack);
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

fn wait(f: &mut Fixture, seconds: f64) {
    let t = now(f) + seconds;
    f.advance_to(t);
}

/// Casts `game_id` as soon as it is castable and lets its cast time pass.
fn cast(f: &mut Fixture, game_id: u32) {
    while f.status(game_id) != SpellStatus::Available {
        wait(f, 0.1);
    }
    let id = f.spell_id(game_id);
    let seconds = {
        let ctx = f.ctx();
        ctx.character.spells().spell(id).cast_time(&ctx)
    };
    f.ctx().cast(id);
    if seconds > 0.0 {
        wait(f, seconds + 0.001);
    }
}

fn cost(f: &mut Fixture, game_id: u32) -> u32 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).resource_cost(&ctx)
}

fn mana(f: &Fixture) -> u32 {
    f.character.resource_level(ResourceType::Mana, 0.0)
}

fn buff(f: &Fixture, game_id: u32) -> Option<&Buff> {
    let spells = f.character.spells();
    spells
        .buff_ids()
        .filter_map(|id| spells.owned_buff(id))
        .find(|buff| buff.spell() == game_id && buff.is_active())
}

/// The recorded casts of the spells called `name` (any rank).
fn attempts(f: &Fixture, name: &str) -> u64 {
    f.character
        .statistics
        .spells()
        .filter(|(key, _)| key.name == name)
        .map(|(_, spell)| spell.total_attempts())
        .sum()
}

fn damage(f: &Fixture, name: &str) -> u64 {
    f.character
        .statistics
        .spells()
        .filter(|(key, _)| key.name == name)
        .map(|(_, spell)| spell.total_damage())
        .sum()
}

fn focus_attempts(f: &Fixture) -> u32 {
    let procs = f.character.spells().procs();
    procs
        .find_by_game_id(ELEMENTAL_FOCUS_PROC)
        .map(|id| procs.get(id).attempts())
        .unwrap()
}

/// Lightning Bolt (20 yd/s) from 30 yd: the mana goes as the cast ends, the damage 1.5 s later.
#[test]
fn lightning_bolt_lands_after_its_flight() {
    let mut f = caster(&[]);
    let before = mana(&f);
    cast(&mut f, LIGHTNING_BOLT);
    assert!(mana(&f) < before, "paid at launch");
    assert_eq!(attempts(&f, "Lightning Bolt"), 0, "in flight");
    wait(&mut f, 1.49);
    assert_eq!(attempts(&f, "Lightning Bolt"), 0);
    wait(&mut f, 0.02);
    assert_eq!(attempts(&f, "Lightning Bolt"), 1);
    assert!(damage(&f, "Lightning Bolt") > 0);
}

/// Flame Shock and Chain Lightning have no missile: they land as they are cast.
#[test]
fn spells_without_a_missile_land_at_once() {
    let mut f = caster(&[]);
    for (game_id, name) in [
        (FLAME_SHOCK, "Flame Shock"),
        (CHAIN_LIGHTNING, "Chain Lightning"),
    ] {
        cast(&mut f, game_id);
        assert_eq!(attempts(&f, name), 1, "{name}");
    }
}

/// At no distance nothing flies: the bolt lands as its cast ends, without a `SpellImpact`.
#[test]
fn nothing_flies_at_no_distance() {
    let mut f = caster(&[]);
    let sim = *f.character.sim();
    f.character.set_sim(SimParams {
        target_distance_yd: Some(0.0),
        ..sim
    });
    let id = f.spell_id(LIGHTNING_BOLT);
    f.ctx().cast(id);
    let events = f.run(now(&f) + 4.0);
    assert!(
        !events
            .iter()
            .any(|kind| matches!(kind, EventKind::SpellImpact { .. }))
    );
    assert_eq!(attempts(&f, "Lightning Bolt"), 1);
}

/// A spell modifier's charge goes at launch: Clearcasting makes the bolt free and is gone while
/// it flies, so an Earth Shock in the meantime costs mana. Elemental Focus rolls as it lands.
#[test]
fn charges_go_at_launch_and_procs_at_impact() {
    let mut f = caster(&[(ELEMENTAL_FOCUS, 1)]);
    f.ctx().trigger_spell(CLEARCASTING, None).unwrap();
    assert!(buff(&f, CLEARCASTING).is_some());
    let before = mana(&f);
    cast(&mut f, LIGHTNING_BOLT);
    assert!(mana(&f) >= before, "free");
    assert!(buff(&f, CLEARCASTING).is_none(), "used by the launch");
    assert_ne!(cost(&mut f, EARTH_SHOCK), 0);
    assert_eq!(focus_attempts(&f), 0, "not landed");
    wait(&mut f, 1.5);
    assert_eq!(focus_attempts(&f), 1);
}

/// Lava Burst's bonus against Flame Shock is decided at launch: a Flame Shock that lands while
/// the Lava Burst flies adds nothing.
#[test]
fn lava_burst_keeps_the_flame_shock_bonus_it_launched_with() {
    let lava = |shock_before: bool, shock_in_flight: bool| {
        let mut f = caster(&[(LIGHTNING_OVERLOAD, 3), (LAVA_BURST_TALENT, 1)]);
        f.rig_rolls(PhysicalAttackResult::Hit);
        if shock_before {
            cast(&mut f, FLAME_SHOCK);
        }
        cast(&mut f, LAVA_BURST);
        if shock_in_flight {
            cast(&mut f, FLAME_SHOCK);
            assert_eq!(attempts(&f, "Lava Burst"), 0, "the shock lands first");
        }
        wait(&mut f, 2.0);
        assert_eq!(attempts(&f, "Lava Burst"), 1);
        damage(&f, "Lava Burst")
    };
    let plain = lava(false, false);
    assert_eq!(lava(false, true), plain);
    assert!(lava(true, false) > plain);
}

/// A bolt still flying when the encounter ends deals nothing.
#[test]
fn a_missile_in_flight_at_the_end_is_lost() {
    let mut f = caster(&[]);
    cast(&mut f, LIGHTNING_BOLT);
    f.engine.end_combat();
    wait(&mut f, 2.0);
    assert_eq!(attempts(&f, "Lightning Bolt"), 0);
}

/// A Lightning Bolt precast is timed by its cast, not its flight: the cast completes at the
/// pull and the bolt lands 1.5 s after it.
#[test]
fn the_precast_completes_at_the_pull() {
    let mut f = super::shaman::with_talents(&[]);
    f.character.set_sim(SimParams::default());
    let spec = "class: SHAMAN\nname: Caster\nattack_mode: magic\nprecast: Lightning Bolt\n\
                cast_if:\n  - name: Lightning Bolt\n";
    f.ctx()
        .set_rotation(Arc::new(serde_yaml::from_str(spec).unwrap()));
    f.ctx().prepare_set_of_combat_iterations();
    f.ctx().reset();
    let start = f.ctx().time_required_to_run_precombat();
    assert!((start - 2.5).abs() < 1e-9, "{start}");
    f.engine.prepare_iteration(-start);
    f.ctx().run_precombat_actions();
    let mut completed = None;
    let mut landed = None;
    while landed.is_none() {
        let event = f.engine.next_event().expect("the bolt lands");
        f.ctx().handle_event(&event);
        match event.kind {
            EventKind::CastComplete { .. } => completed = completed.or(Some(event.time)),
            EventKind::SpellImpact { .. } => landed = Some(event.time),
            _ => {}
        }
    }
    assert!(completed.unwrap().abs() < 1e-9, "{completed:?}");
    assert!((landed.unwrap() - 1.5).abs() < 1e-9, "{landed:?}");
}

/// A melee rotation's missiles fly 5 yd: Holy Wrath (20 yd/s) lands 0.25 s after its cast.
#[test]
fn melee_missiles_fly_five_yards() {
    let mut f = super::paladin_seals::pulled(&[]);
    f.character.set_sim(SimParams::default());
    f.target.set_creature_type(CreatureType::Undead);
    cast(&mut f, HOLY_WRATH);
    assert_eq!(attempts(&f, "Holy Wrath"), 0, "in flight");
    wait(&mut f, 0.24);
    assert_eq!(attempts(&f, "Holy Wrath"), 0);
    wait(&mut f, 0.02);
    assert_eq!(attempts(&f, "Holy Wrath"), 1);
}

/// Searing Totem's attack (19 yd/s) flies from beside the caster: 30 yd take 1.58 s.
#[test]
fn the_totems_attack_flies_too() {
    let mut f = caster(&[]);
    cast(&mut f, SEARING_TOTEM);
    let attacked = loop {
        let event = f.engine.next_event().expect("the totem attacks");
        f.ctx().handle_event(&event);
        if matches!(event.kind, EventKind::TotemAttack { .. }) {
            break event.time;
        }
    };
    assert_eq!(attempts(&f, "Searing Totem Attack"), 0, "in flight");
    f.advance_to(attacked + 30.0 / 19.0 - 0.01);
    assert_eq!(attempts(&f, "Searing Totem Attack"), 0);
    f.advance_to(attacked + 30.0 / 19.0 + 0.01);
    assert_eq!(attempts(&f, "Searing Totem Attack"), 1);
}
