//! Description tests, on the descriptions of the shipped talent spells.

use super::*;
use crate::spell::SpellEffectName;
use crate::spell::record::EffectRecord;

/// A spell `id` described by `description`, with effects of the base points `points`.
fn spell(id: u32, description: &str, points: &[f32]) -> SpellRecord {
    let mut record = SpellRecord::new(id, "Test");
    record.description = description.to_owned();
    record.effects = points
        .iter()
        .enumerate()
        .map(|(index, &points)| {
            let mut effect = EffectRecord::new(index as u32, SpellEffectName::ApplyAura);
            effect.base_points = points;
            effect
        })
        .collect();
    record
}

fn no_spells(_: u32) -> Option<SpellRecord> {
    None
}

fn text(record: &SpellRecord, values: &[(u32, f64)]) -> String {
    describe(record, values, &no_spells)
}

#[test]
fn effect_values_at_a_rank() {
    let cruelty = spell(
        12320,
        "Improves your chance to get a critical strike with melee attacks by $s1%.",
        &[1.0],
    );
    assert_eq!(
        text(&cruelty, &[]),
        "Improves your chance to get a critical strike with melee attacks by 1%."
    );
    assert_eq!(
        text(&cruelty, &[(0, 5.0)]),
        "Improves your chance to get a critical strike with melee attacks by 5%."
    );
    let anger = spell(
        12296,
        "Generates $m2 Rage every $m3 sec, and reduces Rage loss by $s4%.",
        &[0.0, 1.0, 3.0, 50.0],
    );
    assert_eq!(
        text(&anger, &[]),
        "Generates 1 Rage every 3 sec, and reduces Rage loss by 50%."
    );
}

#[test]
fn a_value_alone_has_no_sign() {
    let sinister = spell(
        13732,
        "Reduces the Energy cost of your Sinister Strike ability by $s1.",
        &[-3.0],
    );
    assert_eq!(
        text(&sinister, &[(0, -5.0)]),
        "Reduces the Energy cost of your Sinister Strike ability by 5."
    );
}

#[test]
fn divided_and_multiplied_values() {
    let heroic = spell(
        12282,
        "Reduces the cost of your Heroic Strike ability by $/10;s1 Rage.",
        &[-10.0],
    );
    assert_eq!(
        text(&heroic, &[(0, -30.0)]),
        "Reduces the cost of your Heroic Strike ability by 3 Rage."
    );
    let gouge = spell(13741, "by $/1000;S1 sec, $*2;m1 twice", &[1500.0]);
    assert_eq!(text(&gouge, &[]), "by 1.5 sec, 3000 twice");
}

#[test]
fn arithmetic_with_decimals() {
    let slam = spell(
        12862,
        "by ${$m1/-1000}.2 sec. In addition, by ${$m3/-1000}.1 sec.",
        &[-250.0, 0.0, -500.0],
    );
    assert_eq!(text(&slam, &[]), "by 0.25 sec. In addition, by 0.5 sec.");
    let lingering = spell(1, "by ${$s1/1000} sec", &[2000.0]);
    assert_eq!(text(&lingering, &[(0, 10000.0)]), "by 10 sec");
    let gore = spell(1, "restore ${$s2}.1% of", &[2.0, 0.5]);
    assert_eq!(text(&gore, &[]), "restore 0.5% of");
    let nested = spell(1, "${($s1+$s2)*-2}", &[1.0, 2.0]);
    assert_eq!(text(&nested, &[]), "-6", "the sign counts in arithmetic");
}

#[test]
fn values_of_other_spells() {
    let mut proc = spell(12964, "", &[10.0]);
    proc.duration_ms = Some(12_000);
    proc.aura_options.proc_charges = 3;
    proc.aura_options.max_stacks = 5;
    let lookup = |id| (id == 12964).then(|| proc.clone());
    let wrath = spell(
        13002,
        "a $m1% chance to generate ${$12964m1/10} Rage for $12964d, $12964n swings, up to \
         $12964u times.",
        &[12.0],
    );
    assert_eq!(
        describe(&wrath, &[(0, 60.0)], &lookup),
        "a 60% chance to generate 1 Rage for 12 sec, 3 swings, up to 5 times."
    );
    // Unknown: as written.
    let unknown = spell(1, "for $99999d and $99999s1.", &[]);
    assert_eq!(
        describe(&unknown, &[], &lookup),
        "for $99999d and $99999s1."
    );
}

#[test]
fn durations_proc_chances_and_periods() {
    let mut wish = spell(12292, "Lasts $d, $h% chance, $n charges.", &[20.0]);
    wish.duration_ms = Some(30_000);
    wish.aura_options.proc_chance = 10;
    wish.aura_options.proc_charges = 3;
    assert_eq!(text(&wish, &[]), "Lasts 30 sec, 10% chance, 3 charges.");
    wish.duration_ms = Some(120_000);
    assert_eq!(text(&wish, &[]), "Lasts 2 min, 10% chance, 3 charges.");
    wish.duration_ms = Some(3_600_000);
    assert_eq!(text(&wish, &[]), "Lasts 1 hr, 10% chance, 3 charges.");

    let mut rend = spell(1, "$o1 damage over $d, every $t1 sec.", &[5.0]);
    rend.duration_ms = Some(15_000);
    rend.effects[0].aura_period_ms = 3000;
    assert_eq!(text(&rend, &[]), "25 damage over 15 sec, every 3 sec.");
}

#[test]
fn plurals_follow_the_last_number() {
    let awards = spell(1, "Awards $s1 combo $lpoint:points;.", &[1.0]);
    assert_eq!(text(&awards, &[]), "Awards 1 combo point.");
    assert_eq!(text(&awards, &[(0, 2.0)]), "Awards 2 combo points.");
    let levels = spell(1, "as though ${$m1/-5} $Llevel:levels; lower", &[-10.0]);
    assert_eq!(text(&levels, &[]), "as though 2 levels lower");
}

#[test]
fn unknown_tokens_stay() {
    let record = spell(1, "Costs $s9 or $?s123[a][b] or $ alone, $", &[1.0]);
    assert_eq!(
        text(&record, &[]),
        "Costs $s9 or $?s123[a][b] or $ alone, $"
    );
}

#[test]
fn radii_and_line_breaks() {
    let mut howl = spell(
        12323,
        "Causes all enemies within $a2 yds to be Dazed.\r\nNext line.",
        &[-50.0],
    );
    howl.effects[0].radius_yd = [0.0, 10.0];
    assert_eq!(
        text(&howl, &[]),
        "Causes all enemies within 10 yds to be Dazed.\nNext line.",
        "the spell's radius when the effect named has none"
    );
}
