//! The Shaman's totems on the shipped data (TASKS.md SH.7): one per element for its lifetime,
//! the aura it gives the shaman or the attacks it makes, Fire Nova from the fire totem.

use super::energy::pull;
use super::paladin::stat;
use super::shaman::with_talents;
use super::*;
use crate::character_spells::SpellHandle;
use crate::effect::EffectHost;
use crate::totem::TotemElement;

const STRENGTH_OF_EARTH: u32 = 25361;
const GRACE_OF_AIR: u32 = 25359;
const WINDFURY_TOTEM: u32 = 10614;
const MANA_SPRING: u32 = 10497;
const SEARING_TOTEM: u32 = 10438;
const MAGMA_TOTEM: u32 = 10587;
const FLAMETONGUE_TOTEM: u32 = 16387;
const FIRE_NOVA: u32 = 408345;

const IMPROVED_FIRE_NOVA: u32 = 104764;
const CALL_OF_FLAME: u32 = 104770;

/// An Orc Shaman with `talents` forced in, a 3.6 s two-handed axe, against a level 60 target
/// without armor, pulled.
fn pulled(talents: &[(u32, u32)]) -> Fixture {
    let mut f = with_talents(talents);
    f.target.set_level(60);
    f.target.set_base_armor(0);
    f.equip(EquipmentSlot::Mainhand, TWO_HAND_AXE);
    f.ctx().prepare_set_of_combat_iterations();
    f.ctx().reset();
    f.character.set_seed(1);
    pull(&mut f);
    f.advance_to(0.01);
    f
}

fn now(f: &Fixture) -> f64 {
    f.engine.current_time()
}

/// Casts `game_id` as soon as it is castable.
fn cast(f: &mut Fixture, game_id: u32) {
    while f.status(game_id) == SpellStatus::OnGcd {
        let t = now(f) + 0.1;
        f.advance_to(t);
    }
    assert_eq!(f.status(game_id), SpellStatus::Available, "{game_id}");
    let id = f.spell_id(game_id);
    f.ctx().cast(id);
}

fn totem(f: &Fixture, element: TotemElement) -> Option<u32> {
    let spells = f.character.spells();
    f.character.totems.get(element).map(|totem| {
        let buff = f.ctx_ref_buff_spell(totem.buff);
        let _ = spells;
        buff
    })
}

fn strength(f: &Fixture) -> u32 {
    stat(f, |s, ctx| s.get_strength(ctx))
}

fn agility(f: &Fixture) -> u32 {
    stat(f, |s, ctx| s.get_agility(ctx))
}

fn damage_named(f: &Fixture, name: &str) -> (u64, u64) {
    f.character
        .statistics
        .spells()
        .filter(|(key, _)| key.name == name)
        .fold((0, 0), |(attempts, damage), (_, spell)| {
            (
                attempts + spell.total_attempts(),
                damage + spell.total_damage(),
            )
        })
}

impl Fixture {
    /// The game id of the spell whose buff `buff` is.
    fn ctx_ref_buff_spell(&self, buff: crate::ids::BuffId) -> u32 {
        self.character
            .spells()
            .owned_buff(buff)
            .expect("an owned buff")
            .spell()
    }
}

/// Strength of Earth Totem: +53 strength (+15 % with the baseline Enhancing Totems: 61) for its
/// 300 s, a 1 s global cooldown; gone when it ends.
#[test]
fn strength_of_earth_totem() {
    let mut f = pulled(&[]);
    let base = strength(&f);
    cast(&mut f, STRENGTH_OF_EARTH);
    assert_eq!(totem(&f, TotemElement::Earth), Some(STRENGTH_OF_EARTH));
    assert_eq!(strength(&f) - base, 61);
    f.advance_to(now(&f) + 0.99);
    assert_eq!(f.status(MANA_SPRING), SpellStatus::OnGcd);
    f.advance_to(now(&f) + 0.02);
    assert_eq!(f.status(MANA_SPRING), SpellStatus::Available, "a 1 s GCD");
    f.advance_to(301.0);
    assert_eq!(totem(&f, TotemElement::Earth), None);
    assert_eq!(strength(&f), base);
}

/// One totem per element: Windfury Totem replaces Grace of Air (+89, 102 with Enhancing
/// Totems); another element's totem stays.
#[test]
fn a_totem_replaces_the_one_of_its_element() {
    let mut f = pulled(&[]);
    let (base_agility, base_strength) = (agility(&f), strength(&f));
    cast(&mut f, GRACE_OF_AIR);
    cast(&mut f, STRENGTH_OF_EARTH);
    assert_eq!(agility(&f) - base_agility, 102);
    cast(&mut f, WINDFURY_TOTEM);
    assert_eq!(totem(&f, TotemElement::Air), Some(WINDFURY_TOTEM));
    assert_eq!(agility(&f), base_agility, "Grace of Air is gone");
    assert_eq!(strength(&f) - base_strength, 61, "Strength of Earth stays");
    // A new iteration starts without totems.
    f.ctx().reset();
    for element in TotemElement::ALL {
        assert_eq!(totem(&f, element), None);
    }
    assert_eq!(strength(&f), base_strength);
}

/// Windfury Totem gives the shaman its proc (+246 attack power, 320 with the baseline Improved
/// Weapon Totems) unless the main hand has Windfury Weapon.
#[test]
fn windfury_totem_on_the_shaman() {
    let proc_enabled = |imbue: Option<EnchantName>| {
        let mut f = pulled(&[]);
        if let Some(imbue) = imbue {
            f.set_temp_enchants(EquipmentSlot::Mainhand, &[imbue]);
        }
        cast(&mut f, WINDFURY_TOTEM);
        let applied = f.character.totems.get(TotemElement::Air).unwrap().applied;
        let base = f.ctx().melee_ap();
        f.ctx().trigger_spell(10610, None).unwrap();
        (applied.is_some(), f.ctx().melee_ap() - base)
    };
    assert_eq!(proc_enabled(None), (true, 320));
    assert!(!proc_enabled(Some(EnchantName::WindfuryWeapon)).0);
    assert!(!proc_enabled(Some(EnchantName::WindfuryTotem)).0);
}

/// Flametongue Totem: every main-hand hit adds 1363 x the weapon speed / 100 fire, +12 % with
/// Improved Weapon Totems: 55 with a 3.6 s two-hander.
#[test]
fn flametongue_totem_burns_by_weapon_speed() {
    let mut f = pulled(&[]);
    cast(&mut f, FLAMETONGUE_TOTEM);
    let Some(SpellHandle::Proc(id)) = f.character.totems.get(TotemElement::Fire).unwrap().applied
    else {
        panic!("Flametongue Totem's proc aura");
    };
    f.rig_rolls(PhysicalAttackResult::Hit);
    let report = f.ctx().perform_proc(id);
    let damage = report.triggered[0].1.attack.unwrap().damage;
    assert_eq!(damage, 55, "1363 x 1.12 x 3.6 / 100");
}

/// Mana Spring: 10 mana every 2 s while it stands.
#[test]
fn mana_spring_restores_mana() {
    let mut f = pulled(&[]);
    cast(&mut f, MANA_SPRING);
    let t = now(&f);
    let spent = f.character.resource_level(ResourceType::Mana, t) - 1000;
    f.character.resource_mut().lose(spent, t);
    let before = f.character.resource_level(ResourceType::Mana, t);
    // 10 s: five ticks (the five-second rule only stops spirit regeneration).
    f.advance_to(t + 10.0);
    let gained = f.character.resource_level(ResourceType::Mana, t + 10.0) - before;
    let spirit_ticks = gained.saturating_sub(50);
    assert!(gained >= 50, "{gained}");
    assert!(spirit_ticks < 100, "{gained}");
}

/// Searing Totem attacks every 2.2 s for its 55 s: 25 attacks of 47 fire (±15 %) on the magic
/// table, with 0.017 of spell power, the last as its time runs out; Magma Totem replacing it
/// stops them.
#[test]
fn searing_totem_attacks() {
    let mut f = pulled(&[]);
    cast(&mut f, SEARING_TOTEM);
    f.advance_to(60.0);
    let (attacks, damage) = damage_named(&f, "Searing Totem Attack");
    assert_eq!(attacks, 25);
    let per_attack = damage as f64 / attacks as f64;
    assert!((30.0..=60.0).contains(&per_attack), "{per_attack}");

    let mut f = pulled(&[]);
    cast(&mut f, SEARING_TOTEM);
    f.advance_to(now(&f) + 5.0);
    cast(&mut f, MAGMA_TOTEM);
    f.advance_to(now(&f) + 30.0);
    let (attacks, _) = damage_named(&f, "Searing Totem Attack");
    assert_eq!(attacks, 2, "until Magma Totem replaced it");
    // Magma Totem: a pulse every 2 s for 20 s.
    let (pulses, damage) = damage_named(&f, "Magma Totem");
    assert_eq!(pulses, 10);
    assert!(damage > 0);
}

/// Fire Nova needs a fire totem; from one it deals 11307's 419 + 3.4 per level above 52 fire,
/// Improved Fire Nova 2/2 +20 % and 4 s off its 10 s cooldown, Call of Flame 3/3 +15 %.
#[test]
fn fire_nova_needs_a_fire_totem() {
    let mut f = pulled(&[]);
    assert_eq!(f.status(FIRE_NOVA), SpellStatus::NoTotem);
    cast(&mut f, STRENGTH_OF_EARTH);
    f.advance_to(now(&f) + 1.5);
    assert_eq!(f.status(FIRE_NOVA), SpellStatus::NoTotem, "an earth totem");
    cast(&mut f, SEARING_TOTEM);
    f.advance_to(now(&f) + 1.5);
    cast(&mut f, FIRE_NOVA);
    let (novas, _) = damage_named(&f, "Fire Nova");
    assert_eq!(novas, 1);
    let id = f.spell_id(FIRE_NOVA);
    let cooldown = {
        let ctx = f.ctx();
        ctx.character.spells().spell(id).cooldown_remaining(&ctx)
    };
    assert!((cooldown - 10.0).abs() < 1e-6, "{cooldown}");

    let mut f = pulled(&[(IMPROVED_FIRE_NOVA, 2), (CALL_OF_FLAME, 3)]);
    cast(&mut f, SEARING_TOTEM);
    f.advance_to(now(&f) + 1.5);
    cast(&mut f, FIRE_NOVA);
    let id = f.spell_id(FIRE_NOVA);
    let cooldown = {
        let ctx = f.ctx();
        ctx.character.spells().spell(id).cooldown_remaining(&ctx)
    };
    assert!((cooldown - 6.0).abs() < 1e-6, "{cooldown}");
    // The same rolls without the talents: the damage differs by the multipliers alone.
    let mut plain = pulled(&[]);
    cast(&mut plain, SEARING_TOTEM);
    plain.advance_to(now(&plain) + 1.5);
    cast(&mut plain, FIRE_NOVA);
    let (talented, base) = (
        damage_named(&f, "Fire Nova").1,
        damage_named(&plain, "Fire Nova").1,
    );
    let ratio = talented as f64 / base as f64;
    assert!((ratio - 1.2 * 1.15).abs() < 0.01, "{talented} / {base}");
}

/// Another shaman's Strength of Earth (an external buff) and the shaman's own do not stack.
#[test]
fn an_external_totem_buff_does_not_stack() {
    let mut f = pulled(&[]);
    let registry = crate::buff::external::ExternalBuffDb::load(
        &super::energy::data().join("external_buffs.yaml"),
    )
    .unwrap();
    let db = std::mem::take(&mut f.db);
    f.ctx().add_external_buffs(&registry, &db);
    f.db = db;
    let base = strength(&f);
    f.ctx()
        .set_external_buff_selected("Strength of Earth Totem", true)
        .unwrap();
    let external = strength(&f) - base;
    assert!(external > 0);
    cast(&mut f, STRENGTH_OF_EARTH);
    assert_eq!(strength(&f) - base, external);
    assert_eq!(
        f.character.totems.get(TotemElement::Earth).unwrap().applied,
        None
    );
}

/// Call of Flame 3/3 and Elemental Fury 5/5 reach Searing Totem's attack: +15 % damage, crits
/// for 200 %.
#[test]
fn fire_talents_reach_the_totems() {
    const ELEMENTAL_FURY: u32 = 104766;
    const CALL_OF_THUNDER: u32 = 104762;
    const ELEMENTAL_ALACRITY: u32 = 104765;
    let mut f = pulled(&[
        (CALL_OF_FLAME, 3),
        (ELEMENTAL_ALACRITY, 3),
        (CALL_OF_THUNDER, 1),
        (ELEMENTAL_FURY, 5),
    ]);
    let id = f.spell_id(10436);
    let ctx = f.ctx();
    let attack = ctx.character.spells().spell(id);
    assert!((attack.damage_mod(&ctx) - 1.15).abs() < 1e-9);
    assert!((attack.spell_crit_damage_mod(&ctx) - 2.0).abs() < 1e-9);
}

/// A rotation keeps a totem up by its buff: Searing Totem again as it runs out.
#[test]
fn a_rotation_keeps_a_totem_up() {
    let mut f = with_talents(&[]);
    let spec = "class: SHAMAN\nname: Totem\nattack_mode: magic\ncast_if:\n  - name: Searing Totem\n    condition: buff_duration \"Searing Totem\" less 1\n";
    f.ctx()
        .set_rotation(Arc::new(serde_yaml::from_str(spec).unwrap()));
    f.ctx().prepare_set_of_combat_iterations();
    pull(&mut f);
    f.advance_to(120.0);
    // One attack every 2.2 s for the whole fight takes three totems (at 0, ~55, ~110).
    let attacks = damage_named(&f, "Searing Totem Attack").0;
    assert!((53..=55).contains(&attacks), "{attacks}");
}
