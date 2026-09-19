//! Character tests: a Warrior built from the test class spec, the shipped race data, a small
//! item db and the test-world spell db. Port of the class-agnostic parts of `Test/Warrior`.

use std::path::Path;
use std::sync::Arc;

use super::context::{CharacterContext, SwingOutcome};
use super::{Character, ClassSpec, SimParams, STANCE_COOLDOWN};
use crate::combat_roll::PhysicalAttackResult;
use crate::engine::{Engine, Event, EventKind};
use crate::ids::{CharId, SpellId};
use crate::item::{EquipmentDb, EquipmentSlot, ItemSpec, WeaponType};
use crate::phase::Phase;
use crate::race::{Race, RaceDb, RaceSpec};
use crate::resource::ResourceType;
use crate::spell::dbc::AuraState;
use crate::spell::record::{EquippedItems, SpellDb};
use crate::spell::test_world::{db, Raid};
use crate::spell::{Hand, SpellHost, SpellResult, SpellStatus};
use crate::stance::Stance;
use crate::target::Target;

const HEROIC_STRIKE: u32 = 78;
const BLOODTHIRST: u32 = 23881;
const BATTLE_STANCE: u32 = 2457;
const BERSERKER_STANCE: u32 = 2458;
const BATTLE_STANCE_PASSIVE: u32 = 21156;
const BERSERKER_STANCE_PASSIVE: u32 = 7381;
const BLOODRAGE: u32 = 2687;
const BLOODRAGE_BUFF: u32 = 29131;
const REVENGE: u32 = 25288;
const SLAM: u32 = 1464;

const SWORD: u32 = 1;
const DAGGER: u32 = 2;
const TWO_HAND_AXE: u32 = 3;
const SHIELD: u32 = 4;

const ITEMS_YAML: &str = r#"
- id: 1
  name: Sword
  phase: 1
  slot: "1H"
  type: SWORD
  quality: EPIC
  req_lvl: 60
  item_lvl: 60
  damage: { min: 80, max: 120, speed: 2.6 }
- id: 2
  name: Dagger
  phase: 1
  slot: "1H"
  type: DAGGER
  quality: EPIC
  req_lvl: 60
  item_lvl: 60
  damage: { min: 40, max: 60, speed: 1.8 }
- id: 3
  name: Axe
  phase: 1
  slot: "2H"
  type: TWOHAND_AXE
  quality: EPIC
  req_lvl: 60
  item_lvl: 60
  damage: { min: 200, max: 300, speed: 3.6 }
- id: 4
  name: Shield
  phase: 1
  slot: OH
  type: SHIELD
  quality: EPIC
  req_lvl: 60
  item_lvl: 60
  damage: { min: 1, max: 1, speed: 1.0 }
  stats: { ARMOR: 2000 }
"#;

pub(crate) fn warrior_class() -> Arc<ClassSpec> {
    let spec: ClassSpec = serde_yaml::from_str(super::class::WARRIOR_YAML).unwrap();
    spec.validate().unwrap();
    Arc::new(spec)
}

pub(crate) fn race(race: Race) -> RaceSpec {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/races.yaml");
    RaceDb::load(&path).unwrap().get(race).clone()
}

fn equipment_db() -> Arc<EquipmentDb> {
    let items: Vec<ItemSpec> = serde_yaml::from_str(ITEMS_YAML).unwrap();
    Arc::new(EquipmentDb::from_specs(items, Vec::new()).unwrap())
}

/// A one-character world.
pub(crate) struct Fixture {
    pub character: Character,
    pub engine: Engine,
    pub target: Target,
    pub raid: Raid,
    pub db: SpellDb,
}

impl Fixture {
    pub fn orc_warrior() -> Self {
        let mut engine = Engine::new();
        engine.prepare_iteration(0.0);
        let character = Character::new(
            CharId(0),
            warrior_class(),
            &race(Race::Orc),
            equipment_db(),
            Phase::MoltenCore,
            SimParams::default(),
            63,
            0,
            0,
        );
        Fixture {
            character,
            engine,
            target: Target::new(63),
            raid: Raid::default(),
            db: db(),
        }
    }

    pub fn ctx(&mut self) -> CharacterContext<'_, Raid> {
        CharacterContext::new(
            &mut self.character,
            &mut self.engine,
            &mut self.target,
            &mut self.raid,
        )
    }

    pub fn learn(&mut self, id: u32) -> SpellId {
        let db = std::mem::take(&mut self.db);
        let added = self.ctx().learn(&db, id);
        self.db = db;
        added.spell.unwrap_or_else(|| panic!("{id} is a proc"))
    }

    pub fn spell_id(&self, game_id: u32) -> SpellId {
        self.character
            .spells()
            .spell_by_game_id(game_id)
            .unwrap_or_else(|| panic!("spell {game_id} not learned"))
    }

    pub fn equip(&mut self, slot: EquipmentSlot, item: u32) {
        self.character.equipment_mut().equip(slot, item).unwrap();
    }

    pub fn status(&mut self, game_id: u32) -> SpellStatus {
        let id = self.spell_id(game_id);
        let ctx = self.ctx();
        ctx.character.spells().spell(id).status(&ctx)
    }

    pub fn rage(&self) -> u32 {
        self.character.resource_level(ResourceType::Rage)
    }

    pub fn set_rage(&mut self, rage: u32) {
        self.character.resource_mut().reset();
        self.character.gain_resource(ResourceType::Rage, rage);
    }

    pub fn advance_to(&mut self, time: f64) {
        self.engine
            .add_event(Event::new(time, EventKind::EncounterEnd));
        while let Some(event) = self.engine.next_event() {
            if event.kind == EventKind::EncounterEnd {
                break;
            }
            self.ctx().handle_event(&event);
        }
    }

    /// Runs every event up to `until`, returning the kinds handled by the character.
    pub fn run(&mut self, until: f64) -> Vec<EventKind> {
        self.engine
            .add_event(Event::new(until, EventKind::EncounterEnd));
        let mut handled = Vec::new();
        while let Some(event) = self.engine.next_event() {
            if event.kind == EventKind::EncounterEnd {
                break;
            }
            if self.ctx().handle_event(&event) {
                handled.push(event.kind);
            }
        }
        handled
    }

    fn rig_rolls(&mut self, result: PhysicalAttackResult) {
        // Force every white / yellow roll to `result` by narrowing the roll range.
        let (min, max) = match result {
            PhysicalAttackResult::Hit => (9999, 10000),
            PhysicalAttackResult::Miss => (0, 1),
            _ => panic!("unsupported rigged result"),
        };
        self.character
            .roll_mut()
            .random_mut()
            .set_new_range(min, max);
    }
}

#[test]
fn base_stats_combine_class_and_race() {
    let f = Fixture::orc_warrior();
    let view = f.target.stat_view();
    let ctx = f.character.stat_context(&view);
    let stats = f.character.stats();
    assert_eq!(stats.get_strength(&ctx), 123, "100 class + 23 Orc");
    assert_eq!(stats.get_agility(&ctx), 77);
    assert_eq!(stats.get_stamina(&ctx), 112);
    assert_eq!(stats.get_intellect(&ctx), 27);
    assert_eq!(stats.get_spirit(&ctx), 48);
    assert_eq!(f.character.melee_ap(&view), 160 + 2 * 123);
    assert_eq!(
        stats.get_mh_crit_chance(&ctx),
        200 + 385,
        "2 % base + 77 agi / 20"
    );
    assert_eq!(f.character.faction(), crate::faction::Faction::Horde);
    assert_eq!(f.character.stance(), Stance::Battle);
    assert_eq!(f.character.player_name(), "You");
    assert_eq!(f.character.clvl(), 60);
    assert_eq!(f.character.resource_type(), ResourceType::Rage);
    assert_eq!(f.rage(), 0);
}

#[test]
#[should_panic(expected = "not available")]
fn unavailable_race_is_rejected() {
    let mut class = (*warrior_class()).clone();
    class.available_races.retain(|r| *r != Race::Gnome);
    Character::new(
        CharId(0),
        Arc::new(class),
        &race(Race::Gnome),
        equipment_db(),
        Phase::MoltenCore,
        SimParams::default(),
        63,
        0,
        0,
    );
}

#[test]
fn race_change_swaps_attributes_and_offsets() {
    let mut class = (*warrior_class()).clone();
    class.race_stat_offsets.insert(
        Race::Human,
        super::StatOffsets {
            spirit: 4,
            stamina: -1,
            ..Default::default()
        },
    );
    let mut f = Fixture::orc_warrior();
    f.character = Character::new(
        CharId(0),
        Arc::new(class),
        &race(Race::Orc),
        equipment_db(),
        Phase::MoltenCore,
        SimParams::default(),
        63,
        0,
        0,
    );
    let db = std::mem::take(&mut f.db);
    f.ctx().set_race(&db, &race(Race::Human));
    f.db = db;
    let view = f.target.stat_view();
    let ctx = f.character.stat_context(&view);
    assert_eq!(f.character.race(), Race::Human);
    assert_eq!(f.character.stats().get_strength(&ctx), 120);
    assert_eq!(f.character.stats().get_spirit(&ctx), 25 + 22 + 4);
    assert_eq!(f.character.stats().get_stamina(&ctx), 90 + 20 - 1);
    let db = std::mem::take(&mut f.db);
    f.ctx().set_race(&db, &race(Race::Orc));
    f.db = db;
    let ctx = f.character.stat_context(&view);
    assert_eq!(f.character.stats().get_spirit(&ctx), 48, "offsets removed");
    assert_eq!(f.character.stats().get_stamina(&ctx), 112);
}

#[test]
fn global_cooldown_timing() {
    let mut f = Fixture::orc_warrior();
    assert!(f.character.action_ready(0.0), "starts off the GCD");
    assert!(!f.character.on_global_cooldown(0.0));
    f.character.start_global_cooldown(0.0);
    assert!(f.character.on_global_cooldown(1.4));
    assert!(!f.character.action_ready(1.4));
    assert_eq!(f.character.time_until_action_ready(1.0), 0.5);
    assert!(f.character.action_ready(1.49995), "rounding tolerance");
    assert!(f.character.action_ready(1.5));
    assert_eq!(f.character.time_until_action_ready(2.0), 0.0);
}

#[test]
#[should_panic(expected = "Action not ready")]
fn starting_the_gcd_while_on_it_panics() {
    let mut f = Fixture::orc_warrior();
    f.character.start_global_cooldown(0.0);
    f.character.start_global_cooldown(1.0);
}

#[test]
fn stance_and_trinket_cooldowns() {
    let mut f = Fixture::orc_warrior();
    assert!(!f.character.on_stance_cooldown(0.0));
    // Without a GCD running, the stance swap lag pushes the GCD 0.5 s forward.
    assert_eq!(f.character.start_stance_cooldown(0.0), Some(0.5));
    assert!(f.character.on_stance_cooldown(0.99));
    assert!(!f.character.on_stance_cooldown(STANCE_COOLDOWN));
    assert!(f.character.on_global_cooldown(0.4));
    assert!(!f.character.on_global_cooldown(0.5));
    // With a longer GCD running, nothing moves.
    f.character.start_global_cooldown(1.0);
    assert_eq!(f.character.start_stance_cooldown(1.0), None);
    assert_eq!(f.character.next_gcd(), 2.5);

    assert!(!f.character.on_trinket_cooldown(0.0));
    f.character.start_trinket_cooldown(10.0, 20.0);
    assert!(f.character.on_trinket_cooldown(29.9));
    assert!(!f.character.on_trinket_cooldown(30.0));
}

#[test]
fn combo_points_cap_at_five() {
    let mut f = Fixture::orc_warrior();
    f.character.gain_combo_points(3);
    f.character.gain_combo_points(3);
    assert_eq!(f.character.combo_points(), 5);
    f.character.spend_combo_points();
    assert_eq!(f.character.combo_points(), 0);
}

#[test]
fn rage_gains_and_losses_go_through_the_resource() {
    let mut f = Fixture::orc_warrior();
    assert_eq!(f.character.gain_resource(ResourceType::Rage, 130), 100);
    assert_eq!(f.character.gain_resource(ResourceType::Mana, 10), 0);
    assert_eq!(f.character.resource_level(ResourceType::Mana), 0);
    assert_eq!(f.character.max_resource_level(ResourceType::Rage), 100);
    f.character.lose_resource(ResourceType::Rage, 40, 0.0);
    assert_eq!(f.rage(), 60);
    // Level 60 conversion: 286 damage -> 9 rage; the off-hand percent scales it.
    assert_eq!(f.character.rage_from_damage(Hand::Mainhand, 286.0), Some(9));
    f.character.adjust_offhand_rage_percent(50);
    assert_eq!(f.character.rage_from_damage(Hand::Offhand, 286.0), Some(14));
    assert_eq!(f.character.rage_from_damage(Hand::Mainhand, 286.0), Some(9));
}

#[test]
fn weapon_damage_formulas() {
    let mut f = Fixture::orc_warrior();
    let view = f.target.stat_view();
    let ap = f64::from(f.character.melee_ap(&view));
    // Unarmed: 1 damage at speed 2.0.
    assert_eq!(
        f.character.avg_mh_damage(&view),
        (1.0 + 2.0 * ap / 14.0).round() as u32
    );
    assert_eq!(f.character.avg_oh_damage(&view), 0);
    assert!((f.character.random_normalized_mh_dmg(&view) - (1.0 + 2.0 * ap / 14.0)).abs() < 1e-9);

    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    assert!(f.character.is_dual_wielding());
    assert_eq!(
        f.character.avg_mh_damage(&view),
        (100.0 + 2.6 * ap / 14.0).round() as u32
    );
    assert_eq!(
        f.character.avg_oh_damage(&view),
        (50.0 + 1.8 * ap / 14.0).round() as u32
    );
    for _ in 0..50 {
        let normalized = f.character.random_normalized_mh_dmg(&view);
        let min = 80.0 + 2.4 * ap / 14.0;
        let max = 119.0 + 2.4 * ap / 14.0;
        assert!(normalized >= min && normalized <= max, "{normalized}");
        let mh = f.character.random_non_normalized_mh_dmg(&view);
        assert!(
            mh >= 80.0 + 2.6 * ap / 14.0 && mh <= 119.0 + 2.6 * ap / 14.0,
            "{mh}"
        );
        let oh = f.character.random_non_normalized_oh_dmg(&view);
        assert!(
            oh >= 40.0 + 1.8 * ap / 14.0 && oh <= 59.0 + 1.8 * ap / 14.0,
            "{oh}"
        );
    }

    f.equip(EquipmentSlot::Mainhand, TWO_HAND_AXE);
    assert!(
        !f.character.is_dual_wielding(),
        "the two-hander unequips the dagger"
    );
    let normalized = f.character.random_normalized_mh_dmg(&view);
    assert!(normalized >= 200.0 + 3.3 * ap / 14.0 && normalized <= 299.0 + 3.3 * ap / 14.0);
    assert_eq!(
        Character::normalized_speed(crate::item::WeaponSlot::OneHand, WeaponType::Dagger),
        1.7
    );
    assert_eq!(
        Character::normalized_speed(crate::item::WeaponSlot::Ranged, WeaponType::Bow),
        2.8
    );
    assert_eq!(f.character.weapon_speed(Hand::Mainhand, &view), Some(3.6));
    assert_eq!(f.character.weapon_speed(Hand::Offhand, &view), None);
    assert_eq!(f.character.weapon_skill(Hand::Mainhand, &view), 300);
}

#[test]
fn random_in_range_is_inclusive() {
    let mut f = Fixture::orc_warrior();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..200 {
        seen.insert(f.character.random_in_range(3.0, 5.0) as u32);
    }
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), vec![3, 4, 5]);
    assert_eq!(f.character.random_in_range(7.0, 7.0), 7.0);
}

#[test]
fn learning_enables_trainable_spells_and_links_stances() {
    let mut f = Fixture::orc_warrior();
    let hs = f.learn(HEROIC_STRIKE);
    assert!(f.character.spells().spell(hs).is_enabled());
    f.learn(BATTLE_STANCE);
    f.learn(BERSERKER_STANCE);
    f.learn(BATTLE_STANCE_PASSIVE);
    f.learn(BERSERKER_STANCE_PASSIVE);
    let battle = f.character.stance_link(Stance::Battle).unwrap();
    assert_eq!(battle.passive, Some(BATTLE_STANCE_PASSIVE));
    assert_eq!(battle.spell, f.spell_id(BATTLE_STANCE));
    assert_eq!(
        f.character.stance_link(Stance::Berserker).unwrap().passive,
        Some(BERSERKER_STANCE_PASSIVE)
    );
    // In Battle Stance only its passive is enabled, whatever the learn order said.
    let battle_passive = f.spell_id(BATTLE_STANCE_PASSIVE);
    let berserker_passive = f.spell_id(BERSERKER_STANCE_PASSIVE);
    assert!(f.character.spells().spell(battle_passive).is_enabled());
    assert!(!f.character.spells().spell(berserker_passive).is_enabled());
    assert!((f.character.stats().get_total_threat_mod() - 0.8).abs() < 1e-9);
}

#[test]
fn stance_swap_moves_the_passives_and_clamps_rage() {
    let mut f = Fixture::orc_warrior();
    f.learn(BATTLE_STANCE);
    f.learn(BERSERKER_STANCE);
    f.learn(BATTLE_STANCE_PASSIVE);
    f.learn(BERSERKER_STANCE_PASSIVE);
    f.learn(BLOODTHIRST);
    f.set_rage(80);
    let view = f.target.stat_view();
    let crit_before = f
        .character
        .stats()
        .get_mh_crit_chance(&f.character.stat_context(&view));

    let berserker = f.spell_id(BERSERKER_STANCE);
    let report = f.ctx().cast(berserker);
    assert_eq!(report.result, SpellResult::Success);
    assert_eq!(f.character.stance(), Stance::Berserker);
    assert_eq!(f.rage(), 0, "no Tactical Mastery: all rage is lost");
    let berserker_passive = f.spell_id(BERSERKER_STANCE_PASSIVE);
    let battle_passive = f.spell_id(BATTLE_STANCE_PASSIVE);
    assert!(f.character.spells().spell(berserker_passive).is_enabled());
    assert!(!f.character.spells().spell(battle_passive).is_enabled());
    let crit_after = f
        .character
        .stats()
        .get_mh_crit_chance(&f.character.stat_context(&view));
    assert_eq!(
        crit_after,
        crit_before + 120,
        "Berserker Stance passive: +3 % aura crit, suppressed by 1.8 % against a +3 target"
    );
    assert!((f.character.stats().get_total_threat_mod() - 0.8).abs() < 1e-9);
    assert!(f.ctx().on_stance_cooldown());
    f.set_rage(100);
    // The stance swap lag pushed the GCD to 0.5 and scheduled the player action there.
    assert_eq!(f.status(BLOODTHIRST), SpellStatus::OnGcd);
    assert!(f
        .engine
        .queue()
        .peek()
        .is_some_and(|e| matches!(e.kind, EventKind::PlayerAction { .. }) && e.time == 0.5));
    f.advance_to(0.6);
    assert_eq!(f.status(BLOODTHIRST), SpellStatus::OnStanceCooldown);
    f.advance_to(1.0);
    assert_eq!(f.status(BLOODTHIRST), SpellStatus::Available);

    // Tactical Mastery keeps some rage on the way back.
    f.advance_to(2.0);
    f.set_rage(50);
    f.character.adjust_stance_rage_retained(10);
    let battle = f.spell_id(BATTLE_STANCE);
    f.ctx().cast(battle);
    assert_eq!(f.character.stance(), Stance::Battle);
    assert_eq!(f.rage(), 10);
    assert!(f.character.spells().spell(battle_passive).is_enabled());
    assert!(!f.character.spells().spell(berserker_passive).is_enabled());
    let crit_back = f
        .character
        .stats()
        .get_mh_crit_chance(&f.character.stat_context(&view));
    assert_eq!(crit_back, crit_before);
}

#[test]
fn casting_spends_rage_and_runs_the_attack_table() {
    let mut f = Fixture::orc_warrior();
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.learn(BLOODTHIRST);
    f.set_rage(100);
    f.rig_rolls(PhysicalAttackResult::Hit);
    let bt = f.spell_id(BLOODTHIRST);
    assert_eq!(f.status(BLOODTHIRST), SpellStatus::Available);
    let report = f.ctx().cast(bt);
    assert_eq!(report.result, SpellResult::Success);
    assert_eq!(report.resource_cost, 30);
    assert_eq!(f.rage(), 70);
    let attack = report.attack.unwrap();
    assert_eq!(attack.result, PhysicalAttackResult::Hit);
    assert!(attack.damage > 0);
    assert!(f.ctx().on_global_cooldown());
    assert_eq!(f.status(BLOODTHIRST), SpellStatus::OnGcd);
}

#[test]
fn swings_generate_rage_and_reschedule() {
    let mut f = Fixture::orc_warrior();
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    f.rig_rolls(PhysicalAttackResult::Hit);
    f.ctx().start_attack();
    assert!(f.character.spells().is_melee_attacking());
    let handled = f.run(0.05);
    assert_eq!(handled.len(), 2, "both hands swing at 0.0: {handled:?}");
    assert!(f.rage() > 0);
    // The next swings are timed by the weapon speeds.
    let now = f.engine.current_time();
    let mh = f.character.spells().mh_attack().next_expected_use(now);
    let oh = f.character.spells().oh_attack().next_expected_use(now);
    assert!((mh - 2.6).abs() < 1e-9, "{mh}");
    assert!((oh - 1.8).abs() < 1e-9, "{oh}");
    // A stale iteration is ignored.
    assert_eq!(f.ctx().mh_swing(0), SwingOutcome::Skipped);
    f.ctx().stop_attack();
    assert_eq!(f.ctx().mh_swing(1), SwingOutcome::Skipped);
}

#[test]
fn attack_speed_changes_retime_pending_swings() {
    let mut f = Fixture::orc_warrior();
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    f.rig_rolls(PhysicalAttackResult::Hit);
    f.ctx().start_attack();
    f.run(0.05);
    f.advance_to(1.0);
    // 1.6 s left on the main hand; +100 % haste halves it.
    crate::effect::EffectHost::increase_melee_attack_speed(&mut f.ctx(), 100);
    let now = f.engine.current_time();
    let mh = f.character.spells().mh_attack().next_expected_use(now);
    assert!((mh - 1.8).abs() < 1e-9, "{mh}");
    let view = f.target.stat_view();
    assert!((f.character.weapon_speed(Hand::Mainhand, &view).unwrap() - 1.3).abs() < 1e-9);
    crate::effect::EffectHost::decrease_melee_attack_speed(&mut f.ctx(), 100);
    let mh = f.character.spells().mh_attack().next_expected_use(now);
    assert!((mh - 2.6).abs() < 1e-9, "{mh}");
    let handled = f.run(2.61);
    assert!(
        handled
            .iter()
            .any(|kind| matches!(kind, EventKind::MainhandMeleeHit { .. })),
        "the re-timed swing lands: {handled:?}"
    );
}

#[test]
fn heroic_strike_replaces_the_next_swing() {
    let mut f = Fixture::orc_warrior();
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    f.learn(HEROIC_STRIKE);
    f.rig_rolls(PhysicalAttackResult::Hit);
    f.set_rage(100);
    let hs = f.spell_id(HEROIC_STRIKE);
    let report = f.ctx().cast(hs);
    assert!(report.queued);
    assert_eq!(f.character.spells().queued_next_swing(), Some(hs));
    assert!(
        !f.character.is_dual_wielding(),
        "a queued swing removes the DW penalty"
    );
    f.ctx().start_attack();
    let outcome = f.ctx().mh_swing(1);
    match outcome {
        SwingOutcome::NextSwingSpell(report) => {
            assert_eq!(report.result, SpellResult::Success);
            assert_eq!(report.resource_cost, 15);
        }
        other => panic!("expected Heroic Strike, got {other:?}"),
    }
    assert_eq!(f.rage(), 85);
    assert_eq!(f.character.spells().queued_next_swing(), None);
    assert!(f.character.is_dual_wielding());

    // Queued but unaffordable: the queue is dropped and the white swing lands.
    f.set_rage(100);
    f.ctx().cast(hs);
    assert_eq!(f.character.spells().queued_next_swing(), Some(hs));
    f.set_rage(0);
    let now = f.engine.current_time();
    let handled = f.run(now + 2.61);
    assert!(handled
        .iter()
        .any(|kind| matches!(kind, EventKind::MainhandMeleeHit { .. })));
    assert_eq!(f.character.spells().queued_next_swing(), None);
    assert!(f.rage() > 0, "the white swing landed and generated rage");
}

#[test]
fn slam_stops_attacks_while_casting_and_resets_the_swings() {
    let mut f = Fixture::orc_warrior();
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.learn(SLAM);
    f.rig_rolls(PhysicalAttackResult::Hit);
    f.set_rage(100);
    f.ctx().start_attack();
    f.run(0.05);
    let slam = f.spell_id(SLAM);
    let report = f.ctx().cast(slam);
    assert!(report.cast_started);
    assert!(!f.character.spells().is_melee_attacking());
    assert!(f.ctx().cast_in_progress());
    let handled = f.run(3.0);
    assert!(handled
        .iter()
        .any(|kind| matches!(kind, EventKind::CastComplete { .. })));
    assert!(!f.ctx().cast_in_progress());
    assert!(f.character.spells().is_melee_attacking(), "attacks resume");
}

#[test]
fn reset_clears_the_iteration_state_and_keeps_passives() {
    let mut f = Fixture::orc_warrior();
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.learn(BATTLE_STANCE);
    f.learn(BATTLE_STANCE_PASSIVE);
    f.learn(BLOODRAGE);
    f.learn(BLOODRAGE_BUFF);
    f.learn(BLOODTHIRST);
    f.set_rage(100);
    f.rig_rolls(PhysicalAttackResult::Hit);
    f.ctx().start_attack();
    let bt = f.spell_id(BLOODTHIRST);
    f.ctx().cast(bt);
    let bloodrage = f.spell_id(BLOODRAGE);
    let report = f.ctx().cast(bloodrage);
    assert_eq!(report.result, SpellResult::Success);
    assert!(f.ctx().aura_active(BLOODRAGE_BUFF));
    f.character.gain_combo_points(2);
    f.character.start_trinket_cooldown(0.0, 30.0);
    assert!((f.character.stats().get_total_threat_mod() - 0.8).abs() < 1e-9);

    f.engine.prepare_iteration(-2.0);
    f.ctx().reset();
    assert_eq!(f.character.stance(), Stance::Caster);
    assert_eq!(f.rage(), 0);
    assert_eq!(f.character.combo_points(), 0);
    assert!(!f.character.on_trinket_cooldown(0.0));
    assert!(!f.character.action_ready(-2.0), "the GCD is armed at -1.5");
    assert!(f.character.action_ready(-1.5));
    assert!(!f.character.spells().is_melee_attacking());
    f.engine.prepare_iteration(0.0);
    assert_eq!(
        f.status(BLOODTHIRST),
        SpellStatus::InsufficientResources,
        "cooldowns are ready at 0, the rage is gone"
    );
    assert!(!f.ctx().aura_active(BLOODRAGE_BUFF));
    let passive = f.spell_id(BATTLE_STANCE_PASSIVE);
    assert!(
        !f.character.spells().spell(passive).is_enabled(),
        "no stance, no stance passive"
    );
    assert!((f.character.stats().get_total_threat_mod() - 1.0).abs() < 1e-9);

    // Back into Battle Stance for the next iteration.
    let battle = f.spell_id(BATTLE_STANCE);
    f.ctx().cast(battle);
    assert_eq!(f.character.stance(), Stance::Battle);
    assert!(f.character.spells().spell(passive).is_enabled());
    assert!((f.character.stats().get_total_threat_mod() - 0.8).abs() < 1e-9);
}

#[test]
fn encounter_start_begins_attacking() {
    let mut f = Fixture::orc_warrior();
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.rig_rolls(PhysicalAttackResult::Hit);
    f.engine.add_event(Event::new(
        0.0,
        EventKind::EncounterStart {
            character: CharId(0),
        },
    ));
    let handled = f.run(0.1);
    assert!(handled.contains(&EventKind::EncounterStart {
        character: CharId(0)
    }));
    assert!(f.character.spells().is_melee_attacking());
    assert!(handled
        .iter()
        .any(|kind| matches!(kind, EventKind::MainhandMeleeHit { .. })));
    assert!(!f.ctx().handle_event(&Event::new(
        0.0,
        EventKind::PlayerAction {
            character: CharId(0)
        }
    )));
    assert!(!f.ctx().handle_event(&Event::new(
        0.0,
        EventKind::EncounterStart {
            character: CharId(3)
        }
    )));
}

#[test]
fn equipped_item_requirements() {
    let mut f = Fixture::orc_warrior();
    let any_melee = EquippedItems {
        class: 2,
        subclass_mask: 173555,
        inv_type_mask: 0,
    };
    let shield = EquippedItems {
        class: 4,
        subclass_mask: 64,
        inv_type_mask: 0,
    };
    let none = EquippedItems {
        class: -1,
        subclass_mask: 0,
        inv_type_mask: 0,
    };
    assert!(f.ctx().equipped_item_matches(&none));
    assert!(!f.ctx().equipped_item_matches(&any_melee));
    assert!(!f.ctx().equipped_item_matches(&shield));
    f.equip(EquipmentSlot::Mainhand, SWORD);
    assert!(f.ctx().equipped_item_matches(&any_melee));
    assert!(!f.ctx().equipped_item_matches(&shield));
    f.equip(EquipmentSlot::Offhand, SHIELD);
    assert!(f.ctx().equipped_item_matches(&shield));
    assert!(!f.character.is_dual_wielding());
}

#[test]
fn aura_states() {
    let mut f = Fixture::orc_warrior();
    f.learn(REVENGE);
    f.set_rage(100);
    f.equip(EquipmentSlot::Mainhand, SWORD);
    assert!(!f.ctx().caster_aura_state(AuraState::Defensive));
    assert_ne!(f.status(REVENGE), SpellStatus::Available);
    f.character.note_avoided_incoming_attack(0.0);
    assert!(f.ctx().caster_aura_state(AuraState::Defensive));
    assert!(f.character.in_defensive_state(4.9));
    assert!(!f.character.in_defensive_state(5.0));
    assert!(!f.ctx().caster_aura_state(AuraState::Enraged));
    assert!(f.ctx().target_aura_state(AuraState::None));
    assert!(!f.ctx().target_aura_state(AuraState::Wounded20Percent));
}

#[test]
fn offhand_damage_percent_scales_the_penalty() {
    let mut f = Fixture::orc_warrior();
    assert_eq!(f.character.spells().oh_attack().offhand_penalty(), 0.5);
    f.character.adjust_offhand_damage_percent(25);
    assert!((f.character.spells().oh_attack().offhand_penalty() - 0.625).abs() < 1e-9);
    f.character.adjust_offhand_damage_percent(-25);
    assert!((f.character.spells().oh_attack().offhand_penalty() - 0.5).abs() < 1e-9);
}

#[test]
fn extra_attacks_are_performed_after_the_cast() {
    let mut f = Fixture::orc_warrior();
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.rig_rolls(PhysicalAttackResult::Hit);
    f.ctx().start_attack();
    f.run(0.05);
    let rage = f.rage();
    f.character.add_extra_attacks(2);
    let reports = f.ctx().perform_extra_attacks();
    assert_eq!(reports.len(), 2);
    assert!(f.rage() > rage);
    assert_eq!(f.character.pending_extra_attacks(), 0);
}

#[test]
fn prepare_set_of_combat_iterations_drops_tables() {
    let mut f = Fixture::orc_warrior();
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.rig_rolls(PhysicalAttackResult::Hit);
    f.ctx().start_attack();
    f.run(0.05);
    f.ctx().prepare_set_of_combat_iterations();
    assert_eq!(f.character.spells().mh_attack().iteration(), 0);
    assert!(!f.character.spells().is_melee_attacking());
}

/// The shipped data end to end: every Warrior and Orc spell learns, the stances link to their
/// passives, the racials are enabled, and an iteration of swings and casts runs.
#[test]
fn shipped_warrior_data_learns_and_runs() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/spells");
    let mut f = Fixture::orc_warrior();
    f.db = SpellDb::load(&dir).expect("shipped spell data loads");
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    let db = std::mem::take(&mut f.db);
    let added = f.ctx().learn_all(&db);
    f.db = db;
    assert!(added.len() > 100, "{} spells", added.len());
    for stance in [Stance::Battle, Stance::Defensive, Stance::Berserker] {
        let link = f
            .character
            .stance_link(stance)
            .unwrap_or_else(|| panic!("{stance:?}"));
        assert!(link.passive.is_some(), "{stance:?} has a stance passive");
    }
    // Racials: Blood Fury (Orc) is enabled, Berserking (Troll) was never learned.
    let blood_fury = f.spell_id(20572);
    assert!(f.character.spells().spell(blood_fury).is_enabled());
    assert!(f.character.spells().spell_by_game_id(20554).is_none());
    // Bloodrage's hidden payload came up with it.
    let payload = f.spell_id(BLOODRAGE_BUFF);
    assert!(f.character.spells().spell(payload).is_enabled());

    f.rig_rolls(PhysicalAttackResult::Hit);
    f.ctx().reset();
    f.engine.prepare_iteration(0.0);
    let battle = f.spell_id(BATTLE_STANCE);
    f.ctx().cast(battle);
    assert_eq!(f.character.stance(), Stance::Battle);
    f.engine.add_event(Event::new(
        0.0,
        EventKind::EncounterStart {
            character: CharId(0),
        },
    ));
    let handled = f.run(10.0);
    let swings = handled
        .iter()
        .filter(|kind| {
            matches!(
                kind,
                EventKind::MainhandMeleeHit { .. } | EventKind::OffhandMeleeHit { .. }
            )
        })
        .count();
    assert!(swings >= 9, "10 s of 2.6 / 1.8 swings: {swings}");
    assert!(f.rage() > 0);
    let bloodrage = f.spell_id(BLOODRAGE);
    let report = f.ctx().cast(bloodrage);
    assert_eq!(report.result, SpellResult::Success);
    assert!(f.ctx().aura_active(BLOODRAGE_BUFF));
}
