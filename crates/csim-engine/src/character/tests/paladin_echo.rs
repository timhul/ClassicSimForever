//! Twist of Light's Echoes on the shipped data (TASKS.md P.7): replacing Seal of Command,
//! Righteousness, Fury or Justice with another seal gives an Echo of the replaced seal, which
//! the next landed white swing uses up to apply that seal once.

use super::energy::pull;
use super::paladin::with_talents;
use super::paladin_seals::{SEAL_OF_COMMAND_TALENT, is_up, now, pulled, seal, seal_proc};
use super::*;
use crate::combat_log::{AuraChange, CombatLogEvent};
use crate::proc::ProcSource;
use crate::spell::Hand;
use crate::statistics::ClassStatistics;

const TWIST_OF_LIGHT: u32 = 105692;
const ECHO_OF_FURY: u32 = 1311701;
const ECHO_OF_COMMAND: u32 = 1311703;
const ECHO_OF_RIGHTEOUSNESS: u32 = 1311704;
const ECHO_OF_JUSTICE: u32 = 1311705;
const ECHOES: [u32; 4] = [
    ECHO_OF_FURY,
    ECHO_OF_COMMAND,
    ECHO_OF_RIGHTEOUSNESS,
    ECHO_OF_JUSTICE,
];

/// A Paladin with Twist of Light (and the Seal of Command talent) at the pull.
fn twisting() -> Fixture {
    pulled(&[(TWIST_OF_LIGHT, 1)])
}

fn echo_up(f: &mut Fixture, echo: u32) -> bool {
    let id = f.spell_id(echo);
    is_up(f, id)
}

/// The damage of the spells called `name` (any rank) in `stats`.
fn damage_named(stats: &ClassStatistics, name: &str) -> u64 {
    stats
        .spells()
        .filter(|(key, _)| key.name == name)
        .map(|(_, spell)| spell.total_damage())
        .sum()
}

/// One main-hand swing now; the damage per spell name it dealt, with what it triggered.
fn swing(f: &mut Fixture) -> ClassStatistics {
    f.ctx().take_statistics();
    f.ctx().perform_swing(Hand::Mainhand);
    f.ctx().take_statistics()
}

/// Seal of Command replaced by Seal of Righteousness before a swing: the swing deals both
/// seals' damage, Command's through its Echo, which it uses up.
#[test]
fn replacing_seal_of_command_before_a_swing_deals_both() {
    let mut f = twisting();
    seal(&mut f, "Seal of Command");
    assert!(!echo_up(&mut f, ECHO_OF_COMMAND));
    seal(&mut f, "Seal of Righteousness");
    assert!(
        echo_up(&mut f, ECHO_OF_COMMAND),
        "the Echo of the replaced seal"
    );
    let stats = swing(&mut f);
    assert!(
        damage_named(&stats, "Seal of Command") > 0,
        "the Echo's strike"
    );
    assert_eq!(damage_named(&stats, "Seal of Righteousness"), 67);
    assert!(!echo_up(&mut f, ECHO_OF_COMMAND), "the swing used the Echo");

    // The next swing has Righteousness alone.
    let stats = swing(&mut f);
    assert_eq!(damage_named(&stats, "Seal of Command"), 0);
    assert_eq!(damage_named(&stats, "Seal of Righteousness"), 67);
}

/// An Echo of Command fires on every swing that uses it: no 7 PPM roll and no wait for the
/// seal's 1 s proc cooldown, which a proc of the seal right before the swing has started.
#[test]
fn echo_of_command_always_fires() {
    let mut f = twisting();
    for round in 0..10 {
        let view = f.target.stat_view();
        f.character.refill_mana(&view);
        let command = seal(&mut f, "Seal of Command");
        seal(&mut f, "Seal of Righteousness");
        // The seal's own proc right before the swing: its 1 s cooldown runs.
        let proc = seal_proc(&f, command);
        f.ctx().perform_proc(proc);
        let stats = swing(&mut f);
        assert!(
            damage_named(&stats, "Seal of Command") > 0,
            "round {round} at {}",
            now(&f)
        );
    }
}

/// Seal of Righteousness and Seal of Fury echo too: the swing after the swap deals their
/// per-swing damage once.
#[test]
fn echoes_of_righteousness_and_fury() {
    let mut f = twisting();
    seal(&mut f, "Seal of Righteousness");
    seal(&mut f, "Seal of the Crusader");
    assert!(echo_up(&mut f, ECHO_OF_RIGHTEOUSNESS));
    let stats = swing(&mut f);
    assert!(damage_named(&stats, "Seal of Righteousness") > 0);
    assert!(!echo_up(&mut f, ECHO_OF_RIGHTEOUSNESS));

    seal(&mut f, "Seal of Fury");
    seal(&mut f, "Seal of the Crusader");
    assert!(echo_up(&mut f, ECHO_OF_FURY));
    let stats = swing(&mut f);
    assert_eq!(damage_named(&stats, "Seal of Fury"), 35);
    assert!(!echo_up(&mut f, ECHO_OF_FURY));
}

/// Seal of Justice gives its Echo, which applies nothing the sim deals; the other seals give
/// none.
#[test]
fn only_four_seals_echo() {
    let mut f = twisting();
    seal(&mut f, "Seal of Justice");
    seal(&mut f, "Seal of Wisdom");
    assert!(echo_up(&mut f, ECHO_OF_JUSTICE));
    swing(&mut f);
    assert!(!echo_up(&mut f, ECHO_OF_JUSTICE));

    for (from, to) in [
        ("Seal of Wisdom", "Seal of the Crusader"),
        ("Seal of the Crusader", "Seal of Command"),
    ] {
        seal(&mut f, from);
        seal(&mut f, to);
        for echo in ECHOES {
            assert!(!echo_up(&mut f, echo), "{from} → {to}: {echo}");
        }
    }
}

/// Only a landed white swing uses the Echo: a weapon strike (Holy Strike's main-hand spell
/// event) or a missed swing leaves it, and applies nothing.
#[test]
fn a_strike_or_a_missed_swing_leaves_the_echo() {
    let mut f = twisting();
    seal(&mut f, "Seal of Command");
    seal(&mut f, "Seal of Righteousness");

    f.ctx().take_statistics();
    f.ctx().run_proc_checks(&[ProcSource::MainhandSpell]);
    f.ctx().consume_charges(ProcSource::MainhandSpell);
    let stats = f.ctx().take_statistics();
    assert_eq!(damage_named(&stats, "Seal of Command"), 0);
    assert!(echo_up(&mut f, ECHO_OF_COMMAND), "a strike leaves the Echo");

    f.rig_rolls(PhysicalAttackResult::Miss);
    let stats = swing(&mut f);
    assert_eq!(damage_named(&stats, "Seal of Command"), 0);
    assert!(echo_up(&mut f, ECHO_OF_COMMAND), "a miss leaves the Echo");

    f.rig_rolls(PhysicalAttackResult::Hit);
    let stats = swing(&mut f);
    assert!(damage_named(&stats, "Seal of Command") > 0);
    assert!(!echo_up(&mut f, ECHO_OF_COMMAND));
}

/// Replacing Command again while its Echo is up keeps one Echo, which applies the seal once.
#[test]
fn a_second_replacement_refreshes_the_echo() {
    let mut f = twisting();
    seal(&mut f, "Seal of Command");
    seal(&mut f, "Seal of Righteousness");
    seal(&mut f, "Seal of Command");
    seal(&mut f, "Seal of Righteousness");
    let echo = f.spell_id(ECHO_OF_COMMAND);
    let buff = f.character.spells().spell(echo).marker_buff().unwrap();
    assert_eq!(f.ctx().buff_ref(buff).charges(), 1);
    swing(&mut f);
    assert!(!echo_up(&mut f, ECHO_OF_COMMAND));
}

/// Without Twist of Light no seal gives an Echo.
#[test]
fn no_echo_without_twist_of_light() {
    let mut f = pulled(&[]);
    seal(&mut f, "Seal of Command");
    seal(&mut f, "Seal of Righteousness");
    for echo in ECHOES {
        assert!(!echo_up(&mut f, echo), "{echo}");
    }
    let stats = swing(&mut f);
    assert_eq!(damage_named(&stats, "Seal of Righteousness"), 67);
    assert_eq!(damage_named(&stats, "Seal of Command"), 0);
}

/// A rotation twisting on the swing timer: Seal of Righteousness right before a swing while
/// Seal of Command is up, Command again once the Echo is used. The swings after a swap deal
/// both seals' damage.
#[test]
fn a_rotation_twists_on_the_swing_timer() {
    let mut f = with_talents(&[(SEAL_OF_COMMAND_TALENT, 1), (TWIST_OF_LIGHT, 1)]);
    f.target.set_level(60);
    f.character.set_seed(1);
    let spec = r#"
class: PALADIN
name: Twist
attack_mode: melee
cast_if:
  - name: Seal of Righteousness
    condition: |-
      buff_duration "Seal of Command" is true
      and variable "time_remaining_swing" less 0.5
  - name: Seal of Command
    condition: |-
      buff_duration "Seal of Command" is false
      and buff_duration "Echo of Command" is false
"#;
    f.ctx()
        .set_rotation(std::sync::Arc::new(serde_yaml::from_str(spec).unwrap()));
    f.ctx().prepare_set_of_combat_iterations();
    f.engine.enable_combat_log();
    pull(&mut f);
    let mut t = 0.0;
    while t < 30.0 {
        t += 0.5;
        f.advance_to(t);
    }
    let log = f.engine.take_combat_log().unwrap();
    let echoes = log
        .entries()
        .iter()
        .filter(|entry| {
            matches!(&entry.event, CombatLogEvent::SpellAura { spell, change: AuraChange::Applied, .. }
                if spell.name == "Echo of Command")
        })
        .count();
    let stats = f.ctx().take_statistics();
    assert!(echoes >= 6, "{echoes} Echoes");
    assert!(damage_named(&stats, "Seal of Command") > 0);
    assert!(damage_named(&stats, "Seal of Righteousness") > 0);
}
