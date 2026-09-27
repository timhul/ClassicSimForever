//! Stealth and the openers on the shipped data: Stealth is form 30, the openers need it, every
//! cast that is not allowed in Stealth ends it, the auto attacks wait for the opener, and
//! Vanish, Preparation and Cutthroat around it.

use super::energy::{NOTHING, rogue};
use super::rogue::{BACKSTAB, SINISTER_STRIKE, buff_of, cast_at, combo_points};
use super::rogue_talents::with_talents;
use super::*;
use crate::effect::EffectHost;
use crate::stance::Stance;

const STEALTH: u32 = 1787;
const AMBUSH: u32 = 11269;
const GARROTE: u32 = 11290;
const CHEAP_SHOT: u32 = 1833;
const VANISH: u32 = 1857;
const PREMEDITATION_SPELL: u32 = 14183;
const PREPARATION_SPELL: u32 = 14185;
const CUTTHROAT_SPELL: u32 = 462708;
const CUTTHROAT_BUFF: u32 = 462707;

const PREMEDITATION: u32 = 105743;
const PREPARATION: u32 = 105746;
const CUTTHROAT: u32 = 105750;
const INITIATIVE: u32 = 105755;

/// Gives `talents` to the fixture regardless of the tree's requirements.
fn force_talents(f: &mut Fixture, talents: &[(u32, u32)]) {
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
}

/// A rogue with a main-hand dagger one second before the pull, every roll rigged to a hit.
fn before_the_pull(talents: &[(u32, u32)], rotation: &str) -> Fixture {
    let mut f = rogue(&[], rotation);
    force_talents(&mut f, talents);
    f.equip(EquipmentSlot::Mainhand, DAGGER);
    f.rig_rolls(PhysicalAttackResult::Hit);
    f.ctx().reset();
    f.engine.prepare_iteration(-1.0);
    f.engine.add_event(Event::new(
        0.0,
        EventKind::EncounterStart {
            character: CharId(0),
        },
    ));
    f
}

/// A rotation that does nothing on its own but attacks in melee.
const MELEE: &str = "attack_mode: melee
cast_if:
  - name: Sinister Strike
    condition: variable \"combo_points\" greater 5
";

fn cast_now(f: &mut Fixture, game_id: u32) -> SpellResult {
    assert_eq!(f.status(game_id), SpellStatus::Available, "{game_id}");
    let id = f.spell_id(game_id);
    f.ctx().cast(id).result
}

/// Casts `game_id` before the pull, where the rotation ignores cooldowns (every cooldown is
/// ready at the pull).
fn cast_precombat(f: &mut Fixture, game_id: u32) -> SpellResult {
    let status = f.status(game_id);
    assert!(
        matches!(status, SpellStatus::Available | SpellStatus::OnCooldown),
        "{game_id}: {status:?}"
    );
    let id = f.spell_id(game_id);
    f.ctx().cast(id).result
}

fn stance(f: &Fixture) -> Stance {
    f.character.stance()
}

fn stealth_active(f: &mut Fixture) -> bool {
    let buff = buff_of(f, STEALTH);
    f.ctx().buff_ref(buff).is_active()
}

fn attacking(f: &Fixture) -> bool {
    f.character.spells().is_melee_attacking()
}

/// The main-hand swings the character handles up to `until`.
fn swings_until(f: &mut Fixture, until: f64) -> usize {
    f.run(until)
        .iter()
        .filter(|kind| matches!(kind, EventKind::MainhandMeleeHit { .. }))
        .count()
}

#[test]
fn stealth_is_a_form_that_the_openers_need() {
    let mut f = before_the_pull(&[], NOTHING);
    for opener in [AMBUSH, GARROTE, CHEAP_SHOT] {
        assert_eq!(f.status(opener), SpellStatus::InCasterForm, "{opener}");
    }
    assert_eq!(cast_precombat(&mut f, STEALTH), SpellResult::Success);
    assert_eq!(stance(&f), Stance::Stealth);
    assert!(stealth_active(&mut f));
    for opener in [AMBUSH, GARROTE, CHEAP_SHOT] {
        assert_eq!(f.status(opener), SpellStatus::Available, "{opener}");
    }
    // Stealth has no global cooldown and no stance cooldown.
    assert!(!f.ctx().on_global_cooldown());
    assert!(!f.ctx().on_stance_cooldown());
}

#[test]
fn stealth_cannot_be_used_in_combat() {
    let mut f = before_the_pull(&[], NOTHING);
    f.advance_to(0.0);
    assert_eq!(f.status(STEALTH), SpellStatus::InCombat);
}

#[test]
fn the_opener_breaks_stealth_and_starts_the_auto_attacks() {
    let mut f = before_the_pull(&[], MELEE);
    cast_precombat(&mut f, STEALTH);
    f.advance_to(0.0);
    // The pull does not start the auto attacks of a rogue in Stealth.
    assert!(!attacking(&f));
    assert_eq!(swings_until(&mut f, 3.0), 0);
    assert_eq!(stance(&f), Stance::Stealth);

    assert_eq!(cast_now(&mut f, AMBUSH), SpellResult::Success);
    assert_eq!(stance(&f), Stance::Caster);
    assert!(!stealth_active(&mut f));
    assert!(attacking(&f));
    assert_eq!(combo_points(&mut f), 1);
    assert!(swings_until(&mut f, 4.0) > 0);
    f.ctx().gain_resource(ResourceType::Energy, 100);
    assert_eq!(f.status(AMBUSH), SpellStatus::InCasterForm);
}

#[test]
fn any_attack_breaks_stealth_but_premeditation_does_not() {
    let mut f = before_the_pull(&[(PREMEDITATION, 1)], MELEE);
    cast_precombat(&mut f, STEALTH);
    assert_eq!(
        cast_precombat(&mut f, PREMEDITATION_SPELL),
        SpellResult::Success
    );
    assert_eq!(stance(&f), Stance::Stealth);
    assert_eq!(combo_points(&mut f), 2);
    f.advance_to(0.0);
    cast_now(&mut f, SINISTER_STRIKE);
    assert_eq!(stance(&f), Stance::Caster);
    assert_eq!(combo_points(&mut f), 3);
    assert!(attacking(&f));
}

/// Initiative 3/3 adds a combo point to an opener cast from Stealth.
#[test]
fn initiative_procs_on_the_opener() {
    let mut f = before_the_pull(&[(INITIATIVE, 3)], NOTHING);
    cast_precombat(&mut f, STEALTH);
    f.advance_to(0.0);
    cast_now(&mut f, AMBUSH);
    assert_eq!(combo_points(&mut f), 2, "Ambush's 1 and Initiative's 1");
}

/// Cheap Shot's stun does not land on a boss (TASKS decision 4), its combo points do.
#[test]
fn cheap_shot_only_awards_its_combo_points() {
    let mut f = before_the_pull(&[], NOTHING);
    cast_precombat(&mut f, STEALTH);
    f.advance_to(0.0);
    assert_eq!(cast_now(&mut f, CHEAP_SHOT), SpellResult::Success);
    assert_eq!(combo_points(&mut f), 2);
    assert_eq!(stance(&f), Stance::Caster);
}

/// The rotation's precombat actions go into Stealth and add Premeditation's points; the opener
/// is the first action at the pull.
#[test]
fn the_rotation_opens_from_stealth() {
    let rotation = "attack_mode: melee
precombat_actions: [Stealth, Premeditation]
cast_if:
  - name: Ambush
  - name: Sinister Strike
    condition: variable \"combo_points\" greater 5
";
    let mut f = before_the_pull(&[(PREMEDITATION, 1)], rotation);
    f.ctx().run_precombat_actions();
    assert_eq!(stance(&f), Stance::Stealth);
    assert_eq!(combo_points(&mut f), 2);
    f.advance_to(0.0);
    assert_eq!(stance(&f), Stance::Caster);
    assert_eq!(combo_points(&mut f), 3);
    assert!(attacking(&f));
}

#[test]
fn vanish_goes_back_to_stealth_for_another_ambush() {
    let mut f = before_the_pull(&[], MELEE);
    f.advance_to(0.0);
    assert!(attacking(&f));
    assert_eq!(cast_now(&mut f, VANISH), SpellResult::Success);
    assert_eq!(stance(&f), Stance::Stealth);
    assert!(!attacking(&f), "Vanish drops the auto attacks");
    assert_eq!(f.status(AMBUSH), SpellStatus::Available);
    cast_at(&mut f, AMBUSH, 1.0);
    assert_eq!(stance(&f), Stance::Caster);
    assert!(attacking(&f));
    f.advance_to(2.0);
    assert_eq!(f.status(VANISH), SpellStatus::OnCooldown);
}

#[test]
fn preparation_finishes_the_other_rogue_cooldowns() {
    let mut f = before_the_pull(&[(PREPARATION, 1)], NOTHING);
    f.advance_to(0.0);
    cast_now(&mut f, VANISH);
    cast_at(&mut f, AMBUSH, 1.0);
    f.advance_to(2.0);
    assert_eq!(f.status(VANISH), SpellStatus::OnCooldown);
    cast_at(&mut f, PREPARATION_SPELL, 2.0);
    f.advance_to(3.0);
    assert_eq!(f.status(VANISH), SpellStatus::Available);
    assert_eq!(f.status(PREPARATION_SPELL), SpellStatus::OnCooldown);
    cast_at(&mut f, VANISH, 3.0);
    assert_eq!(stance(&f), Stance::Stealth);
}

/// Cutthroat: a landed Backstab may let the next Ambush skip Stealth, once.
#[test]
fn cutthroat_lets_one_ambush_skip_stealth() {
    let mut f = with_talents(&[(CUTTHROAT, 5)]);
    f.equip(EquipmentSlot::Mainhand, DAGGER);
    let buff = buff_of(&f, CUTTHROAT_BUFF);
    assert_eq!(f.status(AMBUSH), SpellStatus::InCasterForm);
    f.ctx().set_spell_effect_value(CUTTHROAT_SPELL, 0, 100.0);
    cast_at(&mut f, SINISTER_STRIKE, 0.0);
    assert!(
        !f.ctx().buff_ref(buff).is_active(),
        "only Backstab gives the charge"
    );
    cast_at(&mut f, BACKSTAB, 1.0);
    assert!(f.ctx().buff_ref(buff).is_active());
    assert_eq!(f.ctx().buff_ref(buff).duration(), Some(10.0));
    f.advance_to(2.0);
    f.ctx().gain_resource(ResourceType::Energy, 100);
    // Other openers still need Stealth.
    assert_eq!(f.status(GARROTE), SpellStatus::InCasterForm);
    cast_at(&mut f, AMBUSH, 2.0);
    assert!(!f.ctx().buff_ref(buff).is_active(), "the charge is used");
    f.advance_to(3.0);
    f.ctx().gain_resource(ResourceType::Energy, 100);
    assert_eq!(f.status(AMBUSH), SpellStatus::InCasterForm);
}
