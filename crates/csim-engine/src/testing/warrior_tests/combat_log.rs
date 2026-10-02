//! The combat log entries the simulation records.

use crate::combat_log::{AuraChange, CombatLogEntry, CombatLogEvent, LogUnit, MissType};
use crate::resource::ResourceType;
use crate::spell::Hand;
use crate::testing::warrior::WarriorTest;

fn test() -> WarriorTest {
    let mut test = WarriorTest::new("CombatLog");
    test.raid.engine_mut().enable_combat_log();
    test
}

/// The entries logged so far; the log keeps recording.
fn entries(test: &mut WarriorTest) -> Vec<CombatLogEntry> {
    let engine = test.raid.engine_mut();
    let log = engine.take_combat_log().expect("the log is enabled");
    engine.enable_combat_log();
    log.entries().to_vec()
}

fn events(test: &mut WarriorTest) -> Vec<CombatLogEvent> {
    entries(test).into_iter().map(|entry| entry.event).collect()
}

#[test]
fn a_white_hit_logs_swing_damage_from_the_character_to_the_target() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_white_crit();
    let report = test.when_swing_is_performed(Hand::Mainhand);

    let entries = entries(&mut test);
    let [entry] = entries.as_slice() else {
        panic!("one entry: {entries:?}")
    };
    assert_eq!(entry.source, LogUnit::Character(test.id));
    assert_eq!(entry.dest, LogUnit::Target);
    let CombatLogEvent::SwingDamage { hand, damage, info } = &entry.event else {
        panic!("a swing: {entry:?}")
    };
    assert_eq!(*hand, Hand::Mainhand);
    assert_eq!(damage.amount, report.attack.damage);
    assert!(damage.critical && !damage.glancing);
    assert_eq!(info.power, Some(ResourceType::Rage));
    assert_eq!(info.current_power, test.rage());
    assert_eq!(info.level, 60);
}

#[test]
fn a_glancing_blow_is_flagged() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_white_glancing_blow();
    test.when_swing_is_performed(Hand::Mainhand);

    let events = events(&mut test);
    assert!(
        matches!(&events[..], [CombatLogEvent::SwingDamage { damage, .. }] if damage.glancing && !damage.critical),
        "{events:?}"
    );
}

#[test]
fn an_avoided_swing_logs_swing_missed() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_white_dodge();
    test.when_swing_is_performed(Hand::Offhand);

    assert_eq!(
        events(&mut test),
        [CombatLogEvent::SwingMissed {
            hand: Hand::Offhand,
            miss: MissType::Dodge,
        }]
    );
}

#[test]
fn a_cast_logs_its_cast_before_its_damage() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_crit();
    test.given_warrior_has_rage(100);
    let report = test.cast("Bloodthirst");

    let events = events(&mut test);
    let CombatLogEvent::SpellCastSuccess { spell, .. } = &events[0] else {
        panic!("the cast first: {events:?}")
    };
    assert_eq!(spell.name, "Bloodthirst");
    assert_eq!(spell.school, 1);
    let damage = events
        .iter()
        .find_map(|event| match event {
            CombatLogEvent::SpellDamage { spell, damage, .. } if spell.name == "Bloodthirst" => {
                Some(damage)
            }
            _ => None,
        })
        .expect("the damage");
    assert_eq!(damage.amount, report.attack.unwrap().damage);
    assert!(damage.critical);
}

#[test]
fn a_dodged_cast_logs_spell_missed() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_dodge();
    test.given_warrior_has_rage(100);
    test.cast("Bloodthirst");

    let events = events(&mut test);
    assert!(
        events.iter().any(|event| matches!(
            event,
            CombatLogEvent::SpellMissed { spell, miss: MissType::Dodge, offhand: false }
                if spell.name == "Bloodthirst"
        )),
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, CombatLogEvent::SpellDamage { .. })),
        "{events:?}"
    );
}

#[test]
fn bloodrage_logs_its_rage_and_its_aura_until_it_fades() {
    let mut test = test();
    test.given_warrior_has_rage(0);
    test.cast("Bloodrage");
    test.when_running_queued_events_until(10.01);

    let entries = entries(&mut test);
    let me = LogUnit::Character(test.id);
    assert!(matches!(
        &entries[0].event,
        CombatLogEvent::SpellCastSuccess { spell, .. } if spell.name == "Bloodrage"
    ));
    let energize: Vec<(bool, u32)> = entries
        .iter()
        .filter_map(|entry| match &entry.event {
            CombatLogEvent::SpellEnergize {
                power: ResourceType::Rage,
                amount,
                periodic,
                ..
            } => {
                assert_eq!((entry.source, entry.dest), (me, me));
                Some((*periodic, *amount))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        energize
            .iter()
            .filter(|(p, _)| !p)
            .map(|(_, a)| a)
            .sum::<u32>(),
        10
    );
    assert_eq!(
        energize
            .iter()
            .filter(|(p, _)| *p)
            .map(|(_, a)| a)
            .sum::<u32>(),
        10
    );

    let auras: Vec<AuraChange> = entries
        .iter()
        .filter_map(|entry| match &entry.event {
            CombatLogEvent::SpellAura {
                change,
                debuff: false,
                ..
            } => Some(*change),
            _ => None,
        })
        .collect();
    assert_eq!(auras.first(), Some(&AuraChange::Applied), "{entries:?}");
    assert_eq!(auras.last(), Some(&AuraChange::Removed), "{entries:?}");
}

#[test]
fn a_proc_logs_its_effects_but_no_cast() {
    let mut test = test();
    test.given_fury_talent_with_rank("Enrage", 5);
    test.given_fury_talent_with_rank("Flurry", 5);
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_white_crit();
    test.when_swing_is_performed(Hand::Mainhand);

    let events = events(&mut test);
    assert!(
        events.iter().any(|event| matches!(
            event,
            CombatLogEvent::SpellAura { spell, change: AuraChange::Applied, debuff: false }
                if spell.name == "Flurry"
        )),
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, CombatLogEvent::SpellCastSuccess { .. })),
        "{events:?}"
    );
}

#[test]
fn nothing_is_logged_unless_enabled() {
    let mut test = WarriorTest::new("CombatLog");
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.when_swing_is_performed(Hand::Mainhand);
    assert!(test.raid.engine_mut().take_combat_log().is_none());
}

/// The queue of an on-next-swing spell is a buff in the sim only: the game logs the strike's
/// cast and damage when the swing lands, no aura.
#[test]
fn a_queued_heroic_strike_logs_no_aura() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    test.given_warrior_has_rage(100);
    test.cast("Heroic Strike");
    assert!(test.buff_is_active("Heroic Strike"));
    test.when_next_swing_spell_lands("Heroic Strike");

    let events = events(&mut test);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, CombatLogEvent::SpellAura { .. })),
        "{events:?}"
    );
    assert!(events.iter().any(|event| matches!(
        event,
        CombatLogEvent::SpellDamage { spell, .. } if spell.name == "Heroic Strike"
    )));
}
