//! The Rogue's talent procs on the shipped data: the conditions the server keeps beside the
//! proc flags (finishers, the spells a proc reacts to, the combo points a finisher spent) and
//! what the procs give.

use super::rogue::{
    BACKSTAB, EVISCERATE, EXPOSE_ARMOR, HEMORRHAGE, RUPTURE, SINISTER_STRIKE, SLICE_AND_DICE,
    buff_of, cast_at, combo_points, pulled, set_combo_points,
};
use super::rogue_talents::with_talents;
use super::*;
use crate::buff::BuffKind;
use crate::proc::ProcSource;

const RUTHLESSNESS: u32 = 105721;
const PUNCTURING_WOUNDS: u32 = 105719;
const INITIATIVE: u32 = 105755;
const IMPROVED_EXPOSE_ARMOR: u32 = 105717;
const PREPARATION: u32 = 105746;
const THOUSAND_CUTS: u32 = 110866;

const RUTHLESSNESS_SPELL: u32 = 14156;
const PUNCTURING_WOUNDS_SPELL: u32 = 1224716;
const INITIATIVE_SPELL: u32 = 13976;
const THOUSAND_CUTS_BUFF: u32 = 1310723;
const GARROTE: u32 = 11290;

/// The chance (out of 10 000) of the proc of `game_id` on an event from `source`.
fn proc_chance(f: &mut Fixture, game_id: u32, source: ProcSource) -> u32 {
    let ctx = f.ctx();
    let spells = ctx.character.spells();
    let id = spells.proc_by_game_id(game_id).expect("a proc");
    spells.procs().get(id).proc_range(source, &ctx)
}

fn proc_sources(f: &Fixture, game_id: u32) -> Vec<ProcSource> {
    let spells = f.character.spells();
    let id = spells.proc_by_game_id(game_id).expect("a proc");
    spells.procs().get(id).sources().to_vec()
}

/// Makes the proc of `game_id` fire every time: its chance effect set to 100 %.
fn certain(f: &mut Fixture, game_id: u32, effect: u32) {
    f.ctx().set_spell_effect_value(game_id, effect, 100.0);
}

fn cost(f: &mut Fixture, game_id: u32) -> u32 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).resource_cost(&ctx)
}

/// Ruthlessness: 20/40/60 % on every finisher that spends its combo points (Slice and Dice
/// too, which neither hits nor deals damage), a combo point after the spend.
#[test]
fn ruthlessness_adds_a_combo_point_after_a_finisher() {
    let mut f = with_talents(&[(RUTHLESSNESS, 3)]);
    assert_eq!(proc_sources(&f, RUTHLESSNESS_SPELL), [ProcSource::Finisher]);
    assert_eq!(
        proc_chance(&mut f, RUTHLESSNESS_SPELL, ProcSource::Finisher),
        6000
    );
    // The passive's aura is the rogue's: the enemy target of its effect is who the payload
    // hits. It used to be a debuff that outlived the iteration.
    let proc = f
        .character
        .spells()
        .proc_by_game_id(RUTHLESSNESS_SPELL)
        .unwrap();
    let buff = f
        .character
        .spells()
        .procs()
        .get(proc)
        .spell()
        .marker_buff()
        .unwrap();
    assert_eq!(f.ctx().buff_ref(buff).kind(), BuffKind::SelfBuff);
    assert_eq!(f.target.debuff_count(), 0);

    certain(&mut f, RUTHLESSNESS_SPELL, 0);
    cast_at(&mut f, SINISTER_STRIKE, 0.0);
    assert_eq!(combo_points(&mut f), 1, "a builder does not trigger it");
    set_combo_points(&mut f, 5);
    let report = cast_at(&mut f, SLICE_AND_DICE, 1.0);
    assert_eq!(report.combo_points_spent, 5);
    assert_eq!(combo_points(&mut f), 1);
    // A missed finisher spends nothing.
    set_combo_points(&mut f, 3);
    f.rig_rolls(PhysicalAttackResult::Miss);
    cast_at(&mut f, EVISCERATE, 2.0);
    assert_eq!(combo_points(&mut f), 3);

    f.ctx().reset();
    f.target.check_clean();
}

/// Puncturing Wounds: 15/30/45 % of an extra combo point on a landed Backstab only.
#[test]
fn puncturing_wounds_adds_a_combo_point_to_backstab() {
    let mut f = with_talents(&[(PUNCTURING_WOUNDS, 3)]);
    f.equip(EquipmentSlot::Mainhand, DAGGER);
    assert_eq!(
        proc_chance(&mut f, PUNCTURING_WOUNDS_SPELL, ProcSource::MainhandSpell),
        4500
    );
    certain(&mut f, PUNCTURING_WOUNDS_SPELL, 1);
    cast_at(&mut f, BACKSTAB, 0.0);
    assert_eq!(combo_points(&mut f), 2);
    cast_at(&mut f, SINISTER_STRIKE, 1.0);
    assert_eq!(combo_points(&mut f), 3);
}

/// Initiative 3/3: an extra combo point on every landed Ambush, Garrote or Cheap Shot. Garrote
/// needs Stealth (not simulated yet): cast without the status check.
#[test]
fn initiative_adds_a_combo_point_to_the_openers() {
    let mut f = with_talents(&[(INITIATIVE, 3)]);
    assert_eq!(
        proc_chance(&mut f, INITIATIVE_SPELL, ProcSource::MainhandSpell),
        10000
    );
    let garrote = f.spell_id(GARROTE);
    f.ctx().cast(garrote);
    assert_eq!(combo_points(&mut f), 2);
    cast_at(&mut f, SINISTER_STRIKE, 1.0);
    assert_eq!(combo_points(&mut f), 3);
}

/// Improved Expose Armor 2/2: 10 energy cheaper, and 2 combo points back from an Expose Armor
/// that spent 5.
#[test]
fn improved_expose_armor_refunds_combo_points_at_five() {
    let mut base = pulled(&[]);
    let mut f = with_talents(&[(IMPROVED_EXPOSE_ARMOR, 2)]);
    assert_eq!(
        cost(&mut f, EXPOSE_ARMOR),
        cost(&mut base, EXPOSE_ARMOR) - 10
    );
    set_combo_points(&mut f, 5);
    cast_at(&mut f, EXPOSE_ARMOR, 0.0);
    assert_eq!(combo_points(&mut f), 2);
    set_combo_points(&mut f, 4);
    cast_at(&mut f, EXPOSE_ARMOR, 1.0);
    assert_eq!(combo_points(&mut f), 0);
    // Another finisher at 5 gets nothing back.
    set_combo_points(&mut f, 5);
    cast_at(&mut f, EVISCERATE, 2.0);
    assert_eq!(combo_points(&mut f), 0);
}

/// Thousand Cuts: each damaging Rupture tick stacks −3 energy on the next Backstab or
/// Hemorrhage, which uses up every stack; other abilities leave them.
#[test]
fn thousand_cuts_discounts_the_next_backstab() {
    let mut f = with_talents(&[(PREPARATION, 1), (THOUSAND_CUTS, 1)]);
    f.equip(EquipmentSlot::Mainhand, DAGGER);
    let buff = buff_of(&f, THOUSAND_CUTS_BUFF);
    assert_eq!(cost(&mut f, BACKSTAB), 60);
    set_combo_points(&mut f, 5);
    cast_at(&mut f, RUPTURE, 0.0);
    f.advance_to(4.5);
    assert_eq!(f.ctx().buff_ref(buff).stacks(), 2, "ticks at 2 and 4 s");
    assert_eq!(cost(&mut f, BACKSTAB), 54);
    assert_eq!(cost(&mut f, HEMORRHAGE), 29);
    cast_at(&mut f, SINISTER_STRIKE, 4.5);
    assert!(f.ctx().buff_ref(buff).is_active());
    let report = cast_at(&mut f, BACKSTAB, 5.5);
    assert_eq!(report.resource_cost, 54);
    assert!(!f.ctx().buff_ref(buff).is_active());
    assert_eq!(cost(&mut f, BACKSTAB), 60);
}
