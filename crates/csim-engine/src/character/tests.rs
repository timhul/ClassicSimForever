//! Character tests: a Warrior built from the test class spec, the shipped race data, a small
//! item db and the test-world spell db. Port of the class-agnostic parts of `Test/Warrior`.

use std::path::Path;
use std::sync::Arc;

use super::context::{CharacterContext, SwingOutcome};
use super::{Character, ClassSpec, STANCE_COOLDOWN, SimParams};
use crate::combat_roll::PhysicalAttackResult;
use crate::enchant::EnchantName;
use crate::engine::{Engine, Event, EventKind};
use crate::ids::{CharId, SpellId};
use crate::item::{EquipmentDb, EquipmentSlot, ItemSpec, WeaponType};
use crate::phase::Phase;
use crate::race::{Race, RaceDb, RaceSpec};
use crate::raid::SharedBuffRegistry;
use crate::resource::ResourceType;
use crate::spell::dbc::AuraState;
use crate::spell::record::{EquippedItems, SpellDb};
use crate::spell::test_world::db;
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
const FIST_WEAPON: u32 = 5;
const MACE: u32 = 6;
const AXE: u32 = 7;
/// A dagger whose damage does not vary.
const EVEN_DAGGER: u32 = 8;

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
- id: 5
  name: Fist Weapon
  phase: 1
  slot: "1H"
  type: FIST
  quality: EPIC
  req_lvl: 60
  item_lvl: 60
  damage: { min: 60, max: 90, speed: 2.0 }
- id: 6
  name: Mace
  phase: 1
  slot: "1H"
  type: MACE
  quality: EPIC
  req_lvl: 60
  item_lvl: 60
  damage: { min: 80, max: 120, speed: 2.6 }
- id: 7
  name: One-Handed Axe
  phase: 1
  slot: "1H"
  type: AXE
  quality: EPIC
  req_lvl: 60
  item_lvl: 60
  damage: { min: 80, max: 120, speed: 2.6 }
- id: 8
  name: Even Dagger
  phase: 1
  slot: "1H"
  type: DAGGER
  quality: EPIC
  req_lvl: 60
  item_lvl: 60
  damage: { min: 50, max: 50, speed: 1.8 }
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

pub(crate) fn equipment_db() -> Arc<EquipmentDb> {
    let items: Vec<ItemSpec> = serde_yaml::from_str(ITEMS_YAML).unwrap();
    Arc::new(EquipmentDb::from_specs(items, Vec::new()).unwrap())
}

/// [`equipment_db`] with the shipped enchants (`data/enchants.yaml`).
pub(crate) fn equipment_db_with_enchants() -> Arc<EquipmentDb> {
    let items: Vec<ItemSpec> = serde_yaml::from_str(ITEMS_YAML).unwrap();
    let mut db = EquipmentDb::from_specs(items, Vec::new()).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/enchants.yaml");
    db.set_enchants(crate::enchant::EnchantDb::load(&path).unwrap());
    Arc::new(db)
}

/// A one-character world.
pub(crate) struct Fixture {
    pub character: Character,
    pub engine: Engine,
    pub target: Target,
    pub raid: SharedBuffRegistry,
    pub db: SpellDb,
}

impl Fixture {
    pub fn orc_warrior() -> Self {
        Self::orc(warrior_class())
    }

    /// An orc of `class`.
    pub fn orc(class: Arc<ClassSpec>) -> Self {
        Self::orc_with(class, equipment_db())
    }

    /// An orc of `class` whose items and enchants are `equipment`'s.
    pub fn orc_with(class: Arc<ClassSpec>, equipment: Arc<EquipmentDb>) -> Self {
        let mut engine = Engine::new();
        engine.prepare_iteration(0.0);
        let character = Character::new(
            CharId(0),
            class,
            &race(Race::Orc),
            equipment,
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
            raid: SharedBuffRegistry::new(),
            db: db(),
        }
    }

    pub fn ctx(&mut self) -> CharacterContext<'_, SharedBuffRegistry> {
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
        let db = std::mem::take(&mut self.db);
        self.ctx().equip(&db, slot, item).unwrap();
        self.db = db;
    }

    /// Replaces the temporary enchants on the item in `slot` (empty scrapes them off),
    /// registering the procs they grant.
    pub fn set_temp_enchants(&mut self, slot: EquipmentSlot, enchants: &[EnchantName]) {
        let db = std::mem::take(&mut self.db);
        self.ctx().set_temp_enchants(&db, slot, enchants).unwrap();
        self.db = db;
    }

    pub fn status(&mut self, game_id: u32) -> SpellStatus {
        let id = self.spell_id(game_id);
        let ctx = self.ctx();
        ctx.character.spells().spell(id).status(&ctx)
    }

    pub fn rage(&self) -> u32 {
        self.character.resource_level(ResourceType::Rage, 0.0)
    }

    pub fn set_rage(&mut self, rage: u32) {
        self.character.resource_mut().reset();
        self.character.gain_resource(ResourceType::Rage, rage, 0.0);
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
fn a_warrior_holds_one_combo_point_that_a_gain_only_refreshes() {
    let mut f = Fixture::orc_warrior();
    f.character.gain_combo_points(1, 10.0);
    f.character.gain_combo_points(1, 13.0);
    assert_eq!(
        f.character.combo_points(13.0),
        1,
        "the second gain does not stack"
    );
    assert_eq!(
        f.character.combo_points(18.99),
        1,
        "the second gain restarted the window"
    );
    assert_eq!(f.character.combo_points(19.0), 0, "6 s after the last gain");

    f.character.gain_combo_points(1, 30.0);
    assert_eq!(f.character.combo_points(30.0), 1);
    f.character.spend_combo_points();
    assert_eq!(f.character.combo_points(30.0), 0);
}

#[test]
fn combo_points_cap_at_five_and_never_lapse_by_default() {
    let mut spec = (*warrior_class()).clone();
    spec.max_combo_points = 5;
    spec.combo_point_duration = None;
    let mut f = Fixture::orc(Arc::new(spec));
    f.character.gain_combo_points(3, 0.0);
    f.character.gain_combo_points(3, 0.0);
    assert_eq!(f.character.combo_points(1000.0), 5);
    f.character.spend_combo_points();
    assert_eq!(f.character.combo_points(0.0), 0);
}

#[test]
fn rage_gains_and_losses_go_through_the_resource() {
    let mut f = Fixture::orc_warrior();
    assert_eq!(f.character.gain_resource(ResourceType::Rage, 130, 0.0), 100);
    assert_eq!(f.character.gain_resource(ResourceType::Mana, 10, 0.0), 0);
    assert_eq!(f.character.resource_level(ResourceType::Mana, 0.0), 0);
    assert_eq!(f.character.max_resource_level(ResourceType::Rage), 100);
    f.character.lose_resource(ResourceType::Rage, 40, 0.0);
    assert_eq!(f.rage(), 60);
}

/// A refund on miss keeps the tenths: a dodged 12 rage Heroic Strike costs 2.4 rage.
#[test]
fn rage_refunds_keep_the_tenths() {
    let mut f = Fixture::orc_warrior();
    f.character.gain_resource(ResourceType::Rage, 50, 0.0);
    f.character.lose_resource(ResourceType::Rage, 12, 0.0);
    f.character
        .refund_resource(ResourceType::Rage, 12.0 * 0.8, 0.0);
    let tenths = |f: &mut Fixture| {
        f.character
            .resource_mut()
            .as_rage_mut()
            .unwrap()
            .current_tenths()
    };
    assert_eq!(tenths(&mut f), 476);
    assert_eq!(f.rage(), 47);
    f.character.refund_resource(ResourceType::Mana, 10.0, 0.0);
    assert_eq!(tenths(&mut f), 476, "a resource the class does not use");
}

/// Swing rage comes from the base speed of the weapon in the hand: 3.46 per second for a
/// one-hander, half that in the off hand (scaled by the off-hand rage percent), 4.5 for a
/// two-hander. A crit doubles it, off-hand percent included.
#[test]
fn swing_rage_follows_the_equipped_weapons() {
    let mut f = Fixture::orc_warrior();
    assert_eq!(
        f.character.swing_rage(Hand::Mainhand, false),
        None,
        "no weapon"
    );
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    let close = |rage: Option<f64>, expected: f64| (rage.unwrap() - expected).abs() < 1e-9;
    assert!(close(
        f.character.swing_rage(Hand::Mainhand, false),
        3.46 * 2.6
    ));
    assert!(close(
        f.character.swing_rage(Hand::Offhand, false),
        1.73 * 1.8
    ));
    f.character.adjust_offhand_rage_percent(50);
    assert!(close(
        f.character.swing_rage(Hand::Offhand, false),
        1.73 * 1.8 * 1.5
    ));
    assert!(close(
        f.character.swing_rage(Hand::Offhand, true),
        1.73 * 1.8 * 1.5 * 2.0
    ));
    assert!(close(
        f.character.swing_rage(Hand::Mainhand, false),
        3.46 * 2.6
    ));

    f.equip(EquipmentSlot::Mainhand, TWO_HAND_AXE);
    assert!(close(f.character.swing_rage(Hand::Mainhand, false), 16.2));
    assert_eq!(f.character.swing_rage(Hand::Offhand, false), None);
    // 16.2 × 2 = 32.4 rage.
    assert_eq!(
        f.character.gain_swing_rage(Hand::Mainhand, true),
        Some(32.4)
    );
    f.set_rage(90);
    assert_eq!(
        f.character.gain_swing_rage(Hand::Mainhand, false),
        Some(10.0),
        "capped"
    );
    assert_eq!(f.rage(), 100);
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
    assert_eq!(f.character.avg_mh_weapon_damage(&view), 1.0);
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
    // Attack power left out.
    assert_eq!(f.character.avg_mh_weapon_damage(&view), 100.0);
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
    assert!(
        f.engine
            .queue()
            .peek()
            .is_some_and(|e| matches!(e.kind, EventKind::PlayerAction { .. }) && e.time == 0.5)
    );
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

/// A queued Heroic Strike does not touch the off hand: it is still scheduled, swings and
/// reschedules.
#[test]
fn the_offhand_keeps_swinging_while_heroic_strike_is_queued() {
    let mut f = Fixture::orc_warrior();
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    f.learn(HEROIC_STRIKE);
    f.rig_rolls(PhysicalAttackResult::Hit);
    f.set_rage(100);
    let hs = f.spell_id(HEROIC_STRIKE);
    assert!(f.ctx().cast(hs).queued);
    f.ctx().start_attack();
    let outcome = f.ctx().oh_swing(1);
    assert!(matches!(outcome, SwingOutcome::Swing(_)), "{outcome:?}");
    assert_eq!(f.character.spells().queued_next_swing(), Some(hs));
    let now = f.engine.current_time();
    let oh = f.character.spells().oh_attack().next_expected_use(now);
    assert!((oh - 1.8).abs() < 1e-9, "{oh}");
    assert!(
        f.character.spells().oh_attack().attack_is_valid(2),
        "rescheduled"
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
    let avg_oh = f.character.avg_oh_damage(&f.target.stat_view());
    assert!(avg_oh > 0);
    let hs = f.spell_id(HEROIC_STRIKE);
    let report = f.ctx().cast(hs);
    assert!(report.queued);
    assert_eq!(f.character.spells().queued_next_swing(), Some(hs));
    assert!(
        f.character.is_dual_wielding(),
        "both weapons are still equipped"
    );
    let view = f.target.stat_view();
    assert_eq!(
        f.character.avg_oh_damage(&view),
        avg_oh,
        "the off hand still hits for its damage"
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
    let marker = f.character.spells().spell(hs).marker_buff().unwrap();
    assert!(!f.ctx().buff_ref(marker).is_active());
    f.set_rage(100);
    f.ctx().cast(hs);
    assert_eq!(f.character.spells().queued_next_swing(), Some(hs));
    assert!(f.ctx().buff_ref(marker).is_active());
    f.set_rage(0);
    let now = f.engine.current_time();
    let handled = f.run(now + 2.61);
    assert!(
        handled
            .iter()
            .any(|kind| matches!(kind, EventKind::MainhandMeleeHit { .. }))
    );
    assert_eq!(f.character.spells().queued_next_swing(), None);
    assert!(!f.ctx().buff_ref(marker).is_active(), "the queue dropped");
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
    assert!(
        handled
            .iter()
            .any(|kind| matches!(kind, EventKind::CastComplete { .. }))
    );
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
    f.character.gain_combo_points(2, f.engine.current_time());
    f.character.start_trinket_cooldown(0.0, 30.0);
    assert!((f.character.stats().get_total_threat_mod() - 0.8).abs() < 1e-9);

    f.engine.prepare_iteration(-2.0);
    f.ctx().reset();
    assert_eq!(f.character.stance(), Stance::Caster);
    assert_eq!(f.rage(), 0);
    assert_eq!(f.character.combo_points(-2.0), 0);
    assert!(!f.character.on_trinket_cooldown(-600.0));
    assert!(
        f.character.action_ready(-600.0),
        "the GCD is ready however long before the pull"
    );
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
    assert!(
        handled
            .iter()
            .any(|kind| matches!(kind, EventKind::MainhandMeleeHit { .. }))
    );
    // A player action is the rotation's; without one it is handled and does nothing.
    assert!(f.ctx().handle_event(&Event::new(
        0.0,
        EventKind::PlayerAction {
            character: CharId(0)
        }
    )));
    assert!(!f.ctx().handle_event(&Event::new(
        0.0,
        EventKind::IncomingDamage {
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
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let mut f = Fixture::orc_warrior();
    f.db = SpellDb::load(&data.join("spells")).expect("shipped spell data loads");
    let classes = super::ClassDb::load(&data.join("classes"), None).unwrap();
    f.character = Character::new(
        CharId(0),
        Arc::clone(classes.get(crate::faction::PlayerClass::Warrior).unwrap()),
        &race(Race::Orc),
        equipment_db(),
        Phase::MoltenCore,
        SimParams::default(),
        63,
        0,
        0,
    );
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

/// The shipped Rogue data end to end: the class loads, only Rogue and Orc spells are learned,
/// the abilities resolve to their highest ranks, and an iteration of auto attacks runs.
#[test]
fn shipped_rogue_data_learns_and_runs() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let classes = super::ClassDb::load(&data.join("classes"), None).unwrap();
    let rogue = Arc::clone(classes.get(crate::faction::PlayerClass::Rogue).unwrap());
    let mut f = Fixture::orc(rogue);
    f.db = SpellDb::load(&data.join("spells")).expect("shipped spell data loads");
    let talents = crate::talent::TalentDb::load(&data.join("talents")).unwrap();
    let tree = Arc::clone(talents.get(crate::faction::PlayerClass::Rogue).unwrap());
    f.ctx()
        .set_talents(crate::talent::CharacterTalents::new(tree));
    f.equip(EquipmentSlot::Mainhand, SWORD);
    f.equip(EquipmentSlot::Offhand, DAGGER);
    let db = std::mem::take(&mut f.db);
    let added = f.ctx().learn_all(&db);
    f.db = db;
    assert!(added.len() > 100, "{} spells", added.len());
    let spells = f.character.spells();
    assert!(
        spells.spell_by_game_id(12294).is_none(),
        "Mortal Strike is a Warrior spell"
    );
    assert!(spells.spell(f.spell_id(20572)).is_enabled(), "Blood Fury");
    for (ability, rank, game_id) in [
        ("Sinister Strike", 8, 11294),
        ("Backstab", 9, 25300),
        ("Eviscerate", 9, 31016),
        ("Slice and Dice", 2, 6774),
    ] {
        let group = spells
            .rank_group(ability)
            .unwrap_or_else(|| panic!("{ability}"));
        assert_eq!(group.max_rank(), rank, "{ability}");
        let highest = group
            .get_max_available_spell_rank(|id| spells.spell(id).is_enabled())
            .unwrap_or_else(|| panic!("{ability} has an enabled rank"));
        assert_eq!(spells.spell(highest).game_id(), game_id, "{ability}");
    }
    let mutilate = f.spell_id(1310707);
    assert!(
        !f.character.spells().spell(mutilate).is_enabled(),
        "Mutilate is a talent"
    );

    f.rig_rolls(PhysicalAttackResult::Hit);
    f.ctx().reset();
    f.engine.prepare_iteration(0.0);
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
    assert_eq!(f.character.resource_type(), ResourceType::Energy);
}

mod energy;
mod rogue;
mod rogue_items;
mod rogue_poisons;
mod rogue_procs;
mod rogue_stealth;
mod rogue_talents;

// ---------------------------------------------------------------- external buffs

mod external_buffs {
    use super::*;
    use crate::buff::BuffKind;
    use crate::buff::external::{ExternalBuffDb, ExternalBuffFile};
    use crate::character::context::ExternalBuffToggleError;
    use crate::faction::{Faction, PlayerClass};
    use crate::spell::dbc::{AuraType, ImplicitTarget, SpellEffectName};
    use crate::spell::record::{EffectRecord, SpellRecord};

    const KINGS: u32 = 25898;
    const JUJU_POWER: u32 = 16323;
    const GIANTS: u32 = 11405;
    const STRENGTH_OF_EARTH: u32 = 25362;
    const SUNDER: u32 = 11597;
    const BATTLE_SHOUT: u32 = 25289;

    const REGISTRY: &str = r#"
buffs:
  - name: Greater Blessing of Kings
    spell: 25898
    faction: ALLIANCE
  - name: Strength of Earth Totem
    spell: 25362
    faction: HORDE
  - name: Juju Power
    spell: 16323
    mutex: strength
  - name: Elixir of Giants
    spell: 11405
    mutex: strength
  - name: Battle Shout
    spell: 25289
    classes: [ROGUE]
debuffs:
  - name: Sunder Armor
    spell: 11597
"#;

    fn aura(
        id: u32,
        name: &str,
        aura: AuraType,
        points: f32,
        misc: i32,
        target: ImplicitTarget,
    ) -> SpellRecord {
        let mut record = SpellRecord::new(id, name);
        record.duration_ms = Some(3_600_000);
        let mut effect = EffectRecord::new(0, SpellEffectName::ApplyAura);
        effect.aura = aura;
        effect.base_points = points;
        effect.misc_value = [misc, 0];
        effect.implicit_target = [target, ImplicitTarget::None];
        record.effects.push(effect);
        record
    }

    /// The Warrior fixture with the external buff records and the registry above.
    fn fixture() -> (Fixture, ExternalBuffDb) {
        let mut f = Fixture::orc_warrior();
        for record in [
            aura(
                KINGS,
                "Greater Blessing of Kings",
                AuraType::ModTotalStatPercentage,
                10.0,
                -1,
                ImplicitTarget::UnitTargetRaid,
            ),
            aura(
                STRENGTH_OF_EARTH,
                "Strength of Earth",
                AuraType::ModStat,
                53.0,
                0,
                ImplicitTarget::UnitCaster,
            ),
            aura(
                JUJU_POWER,
                "Juju Power",
                AuraType::ModStat,
                30.0,
                0,
                ImplicitTarget::UnitCaster,
            ),
            aura(
                GIANTS,
                "Greater Strength",
                AuraType::ModStat,
                25.0,
                0,
                ImplicitTarget::UnitCaster,
            ),
        ] {
            f.db.add(None, record).unwrap();
        }
        // Sunder Armor r5 (-450 armor, 5 stacks) and Battle Shout r7 are Warrior spells of the
        // test world already.
        assert_eq!(f.db.get(SUNDER).unwrap().aura_options.max_stacks, 5);
        assert!(f.db.get(BATTLE_SHOUT).unwrap().applies_aura());
        f.target.set_base_armor(3731);
        let file: ExternalBuffFile = serde_yaml::from_str(REGISTRY).unwrap();
        let registry = ExternalBuffDb::from_file(file).unwrap();
        registry.validate(&f.db).unwrap();
        let db = std::mem::take(&mut f.db);
        f.ctx().add_external_buffs(&registry, &db);
        f.db = db;
        (f, registry)
    }

    fn strength(f: &Fixture) -> u32 {
        let view = f.target.stat_view();
        let ctx = f.character.stat_context(&view);
        f.character.stats().get_strength(&ctx)
    }

    #[test]
    fn the_class_is_offered_its_entries_as_hidden_permanent_buffs() {
        let (f, _) = fixture();
        let general = f.character.external_buffs();
        let names: Vec<&str> = general
            .entries()
            .iter()
            .map(|e| e.spec.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Greater Blessing of Kings",
                "Strength of Earth Totem",
                "Juju Power",
                "Elixir of Giants",
                "Sunder Armor"
            ],
            "Battle Shout is for other classes"
        );
        let sunder = general.get("Sunder Armor").unwrap();
        assert!(sunder.debuff);
        assert_eq!(sunder.stacks, 5, "a stacking debuff is kept fully stacked");
        assert!(!sunder.selected);
        let buff = f.character.spells().owned_buff(sunder.buff).unwrap();
        assert_eq!(buff.kind(), BuffKind::External);
        assert!(buff.is_permanent());
        assert!(buff.is_hidden());
        assert!(buff.is_enabled());
        assert!(buff.instance_id().is_some());
        assert_eq!(buff.name(), "Sunder Armor");
        assert_eq!(buff.spell(), SUNDER);
        assert!(
            !f.character.spells().is_buff_enabled(sunder.buff),
            "not among the enabled buffs (never found by name, never consumes charges)"
        );
        assert!(
            f.character
                .spells()
                .owned_buff_by_name("Sunder Armor")
                .is_none()
        );
        let giants = general.get("Elixir of Giants").unwrap();
        assert_eq!(giants.stacks, 1);
        let buff = f.character.spells().owned_buff(giants.buff).unwrap();
        assert_eq!(
            buff.name(),
            "Elixir of Giants",
            "the registry name, not the aura's"
        );
        assert_eq!(buff.canonical_name(), "Elixir of Giants (11405)");
    }

    #[test]
    fn toggling_applies_and_removes_the_auras() {
        let (mut f, _) = fixture();
        assert_eq!(strength(&f), 123);
        assert_eq!(f.ctx().toggle_external_buff("Juju Power"), Ok(true));
        assert_eq!(strength(&f), 153);
        assert!(f.character.external_buffs().is_selected("Juju Power"));
        assert_eq!(
            f.character.external_buffs().selected_buffs(),
            ["Juju Power"]
        );
        assert!(f.ctx().aura_active(JUJU_POWER));

        // Selecting again changes nothing; deselecting removes the auras once.
        assert_eq!(
            f.ctx().set_external_buff_selected("Juju Power", true),
            Ok(true)
        );
        assert_eq!(strength(&f), 153);
        assert_eq!(f.ctx().toggle_external_buff("Juju Power"), Ok(false));
        assert_eq!(strength(&f), 123);
        assert!(!f.ctx().aura_active(JUJU_POWER));
        assert!(f.character.external_buffs().selected_buffs().is_empty());
        assert_eq!(
            f.ctx().set_external_buff_selected("Juju Power", false),
            Ok(false)
        );
        assert_eq!(strength(&f), 123);
    }

    #[test]
    fn a_stacking_debuff_is_applied_once_per_stack_on_the_target() {
        let (mut f, _) = fixture();
        let base_armor = f.target.armor();
        assert_eq!(f.ctx().toggle_external_buff("Sunder Armor"), Ok(true));
        assert_eq!(f.target.armor(), base_armor - 5 * 450);
        let sunder = f.character.external_buffs().get("Sunder Armor").unwrap();
        let buff = f.character.spells().owned_buff(sunder.buff).unwrap();
        assert_eq!(buff.stacks(), 5);
        assert_eq!(
            f.character.external_buffs().selected_debuffs(),
            ["Sunder Armor"]
        );
        assert!(
            f.target.debuff_count() == 0,
            "an external debuff takes no debuff slot"
        );
        assert_eq!(f.ctx().toggle_external_buff("Sunder Armor"), Ok(false));
        assert_eq!(f.target.armor(), base_armor);
    }

    #[test]
    fn a_selected_external_debuff_stands_in_for_the_own_one() {
        use crate::rotation::ConditionContext;
        let (mut f, _) = fixture();
        f.rig_rolls(PhysicalAttackResult::Hit);
        let sunder = f.learn(SUNDER);
        let own = f
            .character
            .spells()
            .buff_by_name("Sunder Armor", 0, &f.raid, |_| true)
            .unwrap();
        let base_armor = f.target.armor();

        // The own debuff is replaced when the external one is selected.
        f.set_rage(100);
        assert_eq!(f.ctx().cast(sunder).result, SpellResult::Success);
        assert_eq!(f.ctx().buff_stacks(&own), 1);
        f.ctx().toggle_external_buff("Sunder Armor").unwrap();
        assert!(!f.ctx().buff_ref(own).is_active());
        assert_eq!(f.target.armor(), base_armor - 5 * 450);

        // Conditions on the own debuff read the external one, and casting adds nothing.
        assert_eq!(f.ctx().buff_stacks(&own), 5);
        assert!(f.ctx().buff_is_active(&own));
        assert_eq!(f.ctx().buff_time_left(&own), f64::MAX);
        f.advance_to(2.0);
        f.set_rage(100);
        f.ctx().cast(sunder);
        assert!(!f.ctx().buff_ref(own).is_active());
        assert_eq!(f.target.armor(), base_armor - 5 * 450);

        // Deselected, the own debuff is back.
        f.ctx().toggle_external_buff("Sunder Armor").unwrap();
        assert_eq!(f.ctx().buff_stacks(&own), 0);
        f.advance_to(4.0);
        f.set_rage(100);
        f.ctx().cast(sunder);
        assert_eq!(f.ctx().buff_stacks(&own), 1);
        assert_eq!(f.target.armor(), base_armor - 450);
    }

    #[test]
    fn mutex_peers_are_deselected() {
        let (mut f, _) = fixture();
        f.ctx().toggle_external_buff("Juju Power").unwrap();
        assert_eq!(strength(&f), 153);
        f.ctx().toggle_external_buff("Elixir of Giants").unwrap();
        assert_eq!(strength(&f), 148, "Juju Power went, Elixir of Giants came");
        assert!(!f.character.external_buffs().is_selected("Juju Power"));
        assert!(f.character.external_buffs().is_selected("Elixir of Giants"));
        f.ctx().toggle_external_buff("Juju Power").unwrap();
        assert_eq!(strength(&f), 153);
        assert!(!f.character.external_buffs().is_selected("Elixir of Giants"));
    }

    #[test]
    fn faction_bound_buffs() {
        let (mut f, _) = fixture();
        assert_eq!(
            f.ctx().toggle_external_buff("Greater Blessing of Kings"),
            Err(ExternalBuffToggleError::WrongFaction(
                "Greater Blessing of Kings".into(),
                "Horde"
            ))
        );
        assert_eq!(
            f.ctx().toggle_external_buff("Flask of the Titans"),
            Err(ExternalBuffToggleError::NotOffered(
                "Flask of the Titans".into()
            ))
        );
        assert_eq!(
            f.ctx().toggle_external_buff("Strength of Earth Totem"),
            Ok(true)
        );
        assert_eq!(strength(&f), 123 + 53);
        let offered: Vec<&str> = f
            .character
            .external_buffs()
            .offered(Faction::Horde)
            .map(|e| e.spec.name.as_str())
            .collect();
        assert!(!offered.contains(&"Greater Blessing of Kings"));
        assert!(offered.contains(&"Sunder Armor"));
    }

    #[test]
    fn selected_externals_survive_the_iteration_reset() {
        let (mut f, _) = fixture();
        f.ctx().toggle_external_buff("Juju Power").unwrap();
        f.ctx().toggle_external_buff("Sunder Armor").unwrap();
        let base_armor = f.target.armor() + 5 * 450;
        f.ctx().prepare_set_of_combat_iterations();
        f.engine.prepare_iteration(-2.0);
        f.ctx().reset();
        assert_eq!(strength(&f), 153);
        assert_eq!(f.target.armor(), base_armor - 5 * 450);
        assert!(f.ctx().aura_active(JUJU_POWER));
        f.engine.prepare_iteration(0.0);
        f.ctx().encounter_start();
        f.run(5.0);
        assert_eq!(strength(&f), 153);
        assert_eq!(f.target.armor(), base_armor - 5 * 450);
        f.ctx().reset();
        assert_eq!(strength(&f), 153);
        assert_eq!(f.target.armor(), base_armor - 5 * 450);
    }

    #[test]
    fn shipped_registry_matches_the_shipped_spells() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let db = SpellDb::load(&data.join("spells")).unwrap();
        let registry = ExternalBuffDb::load(&data.join("external_buffs.yaml")).unwrap();
        registry.validate(&db).unwrap();
        assert_eq!(registry.buffs().len(), 21);
        assert_eq!(registry.debuffs().len(), 4);
        for spec in registry.entries() {
            assert!(
                !db.is_learnable(spec.spell)
                    || db.class_of(spec.spell) == Some(Some(PlayerClass::Warrior)),
                "{}: the aura is an externals.yaml record or a Warrior spell",
                spec.name
            );
        }

        let mut f = Fixture::orc_warrior();
        f.target.set_base_armor(5000);
        f.ctx().add_external_buffs(&registry, &db);
        assert_eq!(
            f.character.external_buffs().entries().len(),
            24,
            "everything but Battle Shout"
        );
        let base_strength = strength(&f);
        let base_health = f.character.max_health(&f.target.stat_view());
        let base_armor = f.target.armor();
        let names: Vec<String> = f
            .character
            .external_buffs()
            .offered(Faction::Horde)
            .map(|e| e.spec.name.clone())
            .collect();
        for name in &names {
            f.ctx().toggle_external_buff(name).unwrap();
        }
        // Mutex groups leave one of each; the numbers come from the records.
        let selected = f.character.external_buffs().selected_buffs();
        assert!(selected.contains(&"Strength of Earth Totem"));
        assert!(selected.contains(&"Blessed Sunfruit"), "the last food wins");
        assert!(!selected.contains(&"Grilled Squid"));
        let selected_debuffs = f.character.external_buffs().selected_debuffs();
        assert!(selected_debuffs.contains(&"Curse of Recklessness"));
        assert!(
            !selected_debuffs.contains(&"Faerie Fire"),
            "Faerie Fire and Curse of Recklessness do not stack"
        );
        assert!(strength(&f) > base_strength + 53 + 30 + 17 + 10);
        assert!(selected.contains(&"Flask of the Titans"));
        assert!(
            f.character.max_health(&f.target.stat_view()) >= base_health + 1200,
            "Flask of the Titans"
        );
        assert_eq!(
            f.target.armor(),
            base_armor - 5 * 450 - 505 - 3 * 165,
            "Sunder x5, Curse of Recklessness, Armor Shatter x3"
        );
        f.ctx().clear_external_buffs();
        assert_eq!(strength(&f), base_strength);
        assert_eq!(f.target.armor(), base_armor);
    }
}

// ---------------------------------------------------------------- talents

/// Talents on the shipped data: the Warrior tree of `data/talents/warrior.yaml` applied to
/// the spells of `data/spells/warrior.yaml`. Port of `Test/Warrior/Talents/*` (the parts
/// whose Forever value exists: Defiance, Two-Handed Weapon Specialization, the Arms tree).
mod talents {
    use super::*;
    use crate::proc::ProcSource;
    use crate::spell::dbc::SpellModOp;
    use crate::talent::{CharacterTalents, TalentDb};

    const DEFLECTION: u32 = 105957;
    const IMPROVED_HEROIC_STRIKE: u32 = 105958;
    const IMPROVED_REND: u32 = 105956;
    const IMPROVED_TACTICAL_MASTERY: u32 = 105954;
    const IMPROVED_OVERPOWER: u32 = 105952;
    const ANGER_MANAGEMENT: u32 = 105951;
    const DEEP_WOUNDS: u32 = 105950;
    const TWO_HANDED_SPEC: u32 = 105948;
    const IMPALE: u32 = 105947;
    const SWEEPING_STRIKES: u32 = 105945;
    const WEAPONMASTER: u32 = 105944;
    const IMPROVED_SLAM: u32 = 110858;
    const IMPROVED_HAMSTRING: u32 = 105942;
    const MORTAL_STRIKE: u32 = 105941;
    const CRUELTY: u32 = 105939;
    const UNBRIDLED_WRATH: u32 = 105937;
    const ANTICIPATION: u32 = 105975;
    const TOUGHNESS: u32 = 105973;
    const DEFIANCE: u32 = 110856;
    const BLOOD_CRAZE: u32 = 105934;
    const BOUNDLESS_RAGE: u32 = 105953;
    const ENRAGE: u32 = 105931;
    const DEATH_WISH: u32 = 105927;
    const FLURRY: u32 = 105928;
    const BLOODTHIRST_TALENT: u32 = 105930;

    const MORTAL_STRIKE_R1: u32 = 12294;
    const MORTAL_STRIKE_R2: u32 = 21551;
    const HEROIC_STRIKE_9: u32 = 25286;
    const UNBRIDLED_WRATH_SPELL: u32 = 12322;
    const FLURRY_SPELL: u32 = 12319;
    const DEFENSIVE_STANCE: u32 = 71;
    const ARMS: u32 = 26;
    const FURY: u32 = 256;
    const PROTECTION: u32 = 257;

    /// An Orc Warrior with the shipped spells and talents attached (before learning).
    fn fixture() -> Fixture {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let mut f = Fixture::orc_warrior();
        f.db = SpellDb::load(&data.join("spells")).expect("shipped spell data loads");
        let classes = super::super::ClassDb::load(&data.join("classes"), None).unwrap();
        f.character = Character::new(
            CharId(0),
            Arc::clone(classes.get(crate::faction::PlayerClass::Warrior).unwrap()),
            &race(Race::Orc),
            equipment_db(),
            Phase::MoltenCore,
            SimParams::default(),
            63,
            0,
            0,
        );
        let talents = TalentDb::load(&data.join("talents")).expect("shipped talent data loads");
        let tree = Arc::clone(talents.get(crate::faction::PlayerClass::Warrior).unwrap());
        f.ctx().set_talents(CharacterTalents::new(tree));
        f.equip(EquipmentSlot::Mainhand, SWORD);
        let db = std::mem::take(&mut f.db);
        f.ctx().learn_all(&db);
        f.db = db;
        f
    }

    fn enabled(f: &Fixture, game_id: u32) -> bool {
        let spells = f.character.spells();
        match spells.handle(game_id) {
            Some(crate::character_spells::SpellHandle::Spell(id)) => spells.spell(id).is_enabled(),
            Some(crate::character_spells::SpellHandle::Proc(id)) => spells.procs().is_enabled(id),
            None => panic!("{game_id} not learned"),
        }
    }

    fn inc(f: &mut Fixture, node: u32) -> bool {
        f.ctx().increment_talent(node)
    }

    fn dec(f: &mut Fixture, node: u32) -> bool {
        f.ctx().decrement_talent(node)
    }

    fn inc_n(f: &mut Fixture, node: u32, n: u32) -> bool {
        (0..n).all(|_| inc(f, node))
    }

    fn rank(f: &Fixture, node: u32) -> u32 {
        f.character.talents().unwrap().rank(node)
    }

    fn mh_crit(f: &Fixture) -> u32 {
        let view = f.target.stat_view();
        f.character
            .stats()
            .get_mh_crit_chance(&f.character.stat_context(&view))
    }

    fn phys_dmg_mod(f: &Fixture) -> f64 {
        let view = f.target.stat_view();
        f.character
            .stats()
            .get_total_physical_damage_mod(&f.character.stat_context(&view))
    }

    fn cost(f: &mut Fixture, game_id: u32) -> u32 {
        let id = f.spell_id(game_id);
        let ctx = f.ctx();
        ctx.character.spells().spell(id).resource_cost(&ctx)
    }

    /// The Arms build the C++ `TestArms::spec_ms` spent, on Forever's tiers (31 points).
    fn spec_ms(f: &mut Fixture) {
        assert!(inc_n(f, IMPROVED_REND, 3));
        assert!(inc_n(f, DEFLECTION, 2));
        assert!(inc_n(f, IMPROVED_TACTICAL_MASTERY, 5));
        assert!(inc_n(f, IMPROVED_OVERPOWER, 2));
        assert!(inc_n(f, ANGER_MANAGEMENT, 1));
        assert!(inc_n(f, DEEP_WOUNDS, 3));
        assert!(inc_n(f, TWO_HANDED_SPEC, 3));
        assert!(inc_n(f, IMPALE, 2));
        assert!(inc_n(f, SWEEPING_STRIKES, 1));
        assert!(inc_n(f, WEAPONMASTER, 5));
        assert!(inc_n(f, IMPROVED_SLAM, 2));
        assert!(inc_n(f, IMPROVED_HAMSTRING, 1));
        assert!(inc_n(f, MORTAL_STRIKE, 1));
    }

    #[test]
    fn talent_granted_spells_wait_for_their_talent() {
        let mut f = fixture();
        // Mortal Strike r1 is flagged trainable in SkillLineAbility, r2 genuinely is: both wait.
        assert!(!enabled(&f, MORTAL_STRIKE_R1));
        assert!(!enabled(&f, MORTAL_STRIKE_R2));
        assert!(!enabled(&f, 23881), "Bloodthirst");
        assert!(!enabled(&f, 12320), "Cruelty");
        assert!(enabled(&f, HEROIC_STRIKE_9), "trainable spells are enabled");
        assert_eq!(f.status(MORTAL_STRIKE_R1), SpellStatus::NotEnabled);

        assert!(!inc(&mut f, MORTAL_STRIKE), "tier 6 locked");
        spec_ms(&mut f);
        assert_eq!(f.character.talents().unwrap().tab_points(ARMS), 31);
        assert!(enabled(&f, MORTAL_STRIKE_R1));
        assert!(enabled(&f, MORTAL_STRIKE_R2), "the rank group came with it");
        assert_ne!(f.status(MORTAL_STRIKE_R1), SpellStatus::NotEnabled);

        // Nothing below comes out while Mortal Strike holds tier 6 at exactly 30 points.
        for node in [
            DEFLECTION,
            IMPROVED_REND,
            IMPROVED_TACTICAL_MASTERY,
            DEEP_WOUNDS,
            IMPALE,
            SWEEPING_STRIKES,
            IMPROVED_SLAM,
        ] {
            assert!(!dec(&mut f, node), "{node}");
        }
        assert!(dec(&mut f, MORTAL_STRIKE));
        assert!(!enabled(&f, MORTAL_STRIKE_R1));
        assert!(!enabled(&f, MORTAL_STRIKE_R2));
        assert!(!dec(&mut f, IMPROVED_REND), "Deep Wounds requires it");
        assert!(dec(&mut f, SWEEPING_STRIKES));
        assert!(
            !dec(&mut f, DEFLECTION),
            "tier 0 is at 5 with tier 1 invested"
        );
        assert!(dec(&mut f, WEAPONMASTER), "tier 5 keeps 25 below it");
        assert!(!dec(&mut f, WEAPONMASTER), "24 would not");
        assert_eq!(f.character.talents().unwrap().points_remaining(), 51 - 28);
    }

    #[test]
    fn talents_attached_after_learning_are_synced() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let mut f = fixture();
        // Detach, learn a fresh character's spells without talents: the talent abilities
        // flagged trainable come up enabled; attaching the tree takes them down again.
        f.character.set_talents_unsynced(None);
        let db = std::mem::take(&mut f.db);
        let mut fresh = Fixture::orc_warrior();
        fresh.db = db;
        fresh.character = Character::new(
            CharId(0),
            Arc::clone(f.character.class()),
            &race(Race::Orc),
            equipment_db(),
            Phase::MoltenCore,
            SimParams::default(),
            63,
            0,
            0,
        );
        let db = std::mem::take(&mut fresh.db);
        fresh.ctx().learn_all(&db);
        fresh.db = db;
        assert!(enabled(&fresh, MORTAL_STRIKE_R1));
        let talents = TalentDb::load(&data.join("talents")).unwrap();
        let mut setup = CharacterTalents::new(Arc::clone(
            talents.get(crate::faction::PlayerClass::Warrior).unwrap(),
        ));
        assert_eq!(setup.increase_to_max_rank(CRUELTY).len(), 5);
        fresh.ctx().set_talents(setup);
        assert!(!enabled(&fresh, MORTAL_STRIKE_R1));
        assert!(!enabled(&fresh, MORTAL_STRIKE_R2));
        assert!(
            enabled(&fresh, 12320),
            "Cruelty had points in the attached setup"
        );
    }

    #[test]
    fn a_talent_spell_learned_after_its_talent_comes_up_at_its_rank() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let mut f = Fixture::orc_warrior();
        f.db = SpellDb::load(&data.join("spells")).unwrap();
        let talents = TalentDb::load(&data.join("talents")).unwrap();
        let mut setup = CharacterTalents::new(Arc::clone(
            talents.get(crate::faction::PlayerClass::Warrior).unwrap(),
        ));
        setup.increase_to_max_rank(CRUELTY);
        setup.increase_to_max_rank(DEFLECTION);
        setup.increase_to_max_rank(IMPROVED_HEROIC_STRIKE);
        f.ctx().set_talents(setup);
        f.equip(EquipmentSlot::Mainhand, SWORD);
        let base_crit = mh_crit(&f);
        f.learn(12320);
        assert!(enabled(&f, 12320));
        assert_eq!(mh_crit(&f), base_crit + cruelty_crit(5));
        f.learn(HEROIC_STRIKE_9);
        f.learn(12282);
        assert_eq!(
            cost(&mut f, HEROIC_STRIKE_9),
            12,
            "Improved Heroic Strike 3/3"
        );
        // Ranks of a granted ability stay down until their talent has points.
        f.learn(MORTAL_STRIKE_R1);
        assert!(!enabled(&f, MORTAL_STRIKE_R1));
        f.learn(MORTAL_STRIKE_R2);
        assert!(!enabled(&f, MORTAL_STRIKE_R2));
    }

    /// Aura crit is suppressed by 1.8 % against the level 63 target
    /// (`Mechanics::suppressed_aura_crit_chance`), so Cruelty's `percent` shows as
    /// `percent × 100 − 180`.
    fn cruelty_crit(percent: u32) -> u32 {
        percent * 100 - 180
    }

    #[test]
    fn rank_values_replace_the_effect_values() {
        let mut f = fixture();
        let base_crit = mh_crit(&f);
        assert!(inc_n(&mut f, CRUELTY, 3));
        assert_eq!(mh_crit(&f), base_crit + cruelty_crit(3), "Cruelty 3 %");
        assert!(inc_n(&mut f, CRUELTY, 2));
        assert_eq!(mh_crit(&f), base_crit + cruelty_crit(5));
        assert!(dec(&mut f, CRUELTY));
        assert_eq!(mh_crit(&f), base_crit + cruelty_crit(4));

        assert_eq!(cost(&mut f, HEROIC_STRIKE_9), 15);
        assert!(inc_n(&mut f, IMPROVED_HEROIC_STRIKE, 2));
        assert_eq!(cost(&mut f, HEROIC_STRIKE_9), 13, "-1 rage per rank");
        assert_eq!(f.character.spell_modifiers().len(), 1);
        assert_eq!(
            f.character
                .spell_modifiers()
                .all()
                .iter()
                .map(|m| (m.op, m.amount))
                .collect::<Vec<_>>(),
            [(SpellModOp::PowerCost0, -20.0)]
        );
        assert!(inc(&mut f, IMPROVED_HEROIC_STRIKE));
        assert_eq!(cost(&mut f, HEROIC_STRIKE_9), 12);
        assert!(f.ctx().clear_talent_tab(ARMS));
        assert_eq!(cost(&mut f, HEROIC_STRIKE_9), 15);
        assert!(f.character.spell_modifiers().is_empty());
        assert_eq!(mh_crit(&f), base_crit + cruelty_crit(4), "Fury untouched");
        assert!(f.ctx().clear_talents());
        assert_eq!(mh_crit(&f), base_crit);
        assert!(!f.ctx().clear_talents(), "nothing left to clear");
    }

    #[test]
    fn a_proc_chance_talent_rolls_with_its_rank_value() {
        let mut f = fixture();
        let proc = f
            .character
            .spells()
            .proc_by_game_id(UNBRIDLED_WRATH_SPELL)
            .unwrap();
        assert!(!f.character.spells().procs().is_enabled(proc));
        assert!(!inc(&mut f, UNBRIDLED_WRATH), "tier 1 needs 5 points");
        assert!(inc_n(&mut f, CRUELTY, 5));
        assert!(inc(&mut f, UNBRIDLED_WRATH));
        assert!(f.character.spells().procs().is_enabled(proc));
        let chance = |f: &mut Fixture| {
            let ctx = f.ctx();
            ctx.character
                .spells()
                .procs()
                .get(proc)
                .spell()
                .proc_chance(&ctx)
        };
        assert!((chance(&mut f) - 0.12).abs() < 1e-9, "rank 1");
        assert!(inc_n(&mut f, UNBRIDLED_WRATH, 4));
        assert!((chance(&mut f) - 0.60).abs() < 1e-9, "rank 5");
        assert!(dec(&mut f, UNBRIDLED_WRATH));
        assert!((chance(&mut f) - 0.48).abs() < 1e-9, "rank 4");
        f.ctx().clear_talents();
        assert!(!f.character.spells().procs().is_enabled(proc));
    }

    /// Unbridled Wrath gives 1 rage per proc with a one-hander, 2 with a two-hander.
    #[test]
    fn unbridled_wrath_gives_double_rage_with_a_two_hander() {
        let mut f = fixture();
        assert!(inc_n(&mut f, CRUELTY, 5));
        assert!(inc_n(&mut f, UNBRIDLED_WRATH, 5));
        // The rage of every swing that procs, over enough swings to proc at 60 %.
        let rage_per_proc = |f: &mut Fixture| {
            let mut gains = Vec::new();
            for _ in 0..20 {
                f.set_rage(0);
                if !f
                    .ctx()
                    .run_proc_checks(&[ProcSource::MainhandSwing])
                    .is_empty()
                {
                    gains.push(f.rage());
                }
            }
            assert!(!gains.is_empty(), "no proc in 20 swings");
            gains
        };
        assert!(!f.character.equipment().has_two_hand_weapon());
        assert!(rage_per_proc(&mut f).iter().all(|&r| r == 1), "one-hander");
        f.equip(EquipmentSlot::Mainhand, TWO_HAND_AXE);
        assert!(f.character.equipment().has_two_hand_weapon());
        assert!(rage_per_proc(&mut f).iter().all(|&r| r == 2), "two-hander");
    }

    /// Flurry's rank value travels through the proc's `TRIGGER_WITH_VALUE` script into the
    /// haste buff: 15 % at rank 3, 25 % at rank 5.
    #[test]
    fn a_scripted_talent_forwards_its_rank_value_to_its_buff() {
        let mut f = fixture();
        let short = f.ctx().spend_talent_points(&[
            (CRUELTY, 5),
            (UNBRIDLED_WRATH, 5),
            (BLOOD_CRAZE, 3),
            (BOUNDLESS_RAGE, 3),
            (ENRAGE, 5),
            (105929, 3), // Precision
            (DEATH_WISH, 1),
            (FLURRY, 3),
        ]);
        assert!(short.is_empty(), "{short:?}");
        assert!(enabled(&f, FLURRY_SPELL));
        f.ctx().reset();
        f.engine.prepare_iteration(0.0);
        let procs = f.ctx().run_proc_checks(&[ProcSource::MeleeCritical]);
        assert!(
            procs.iter().any(
                |(id, _)| f.character.spells().procs().get(*id).spell().game_id() == FLURRY_SPELL
            ),
            "Flurry procs on a crit: {procs:?}"
        );
        let haste = f.character.stats().get_melee_attack_speed_mod();
        assert!((haste - 1.15).abs() < 1e-9, "rank 3: {haste}");
        assert!(inc_n(&mut f, FLURRY, 2));
        f.ctx().reset();
        f.engine.prepare_iteration(0.0);
        f.ctx().run_proc_checks(&[ProcSource::MeleeCritical]);
        let haste = f.character.stats().get_melee_attack_speed_mod();
        assert!((haste - 1.25).abs() < 1e-9, "rank 5: {haste}");
    }

    /// Port of `TestTwoHandedWeaponSpecialization`: 1 % per rank (3 ranks in Forever), only
    /// while a two-hander is equipped.
    #[test]
    fn two_handed_weapon_specialization_needs_a_two_hander() {
        let mut f = fixture();
        assert!((phys_dmg_mod(&f) - 1.0).abs() < 1e-9);
        assert!(inc_n(&mut f, IMPROVED_REND, 3));
        assert!(inc_n(&mut f, DEFLECTION, 2));
        assert!(inc_n(&mut f, IMPROVED_TACTICAL_MASTERY, 5));
        assert!(inc_n(&mut f, IMPROVED_OVERPOWER, 2));
        assert!(inc_n(&mut f, DEEP_WOUNDS, 3));
        assert!(inc(&mut f, TWO_HANDED_SPEC));
        assert!((phys_dmg_mod(&f) - 1.0).abs() < 1e-9, "sword equipped");
        f.equip(EquipmentSlot::Mainhand, TWO_HAND_AXE);
        f.ctx().reevaluate_passives();
        assert!((phys_dmg_mod(&f) - 1.01).abs() < 1e-9);
        assert!(inc(&mut f, TWO_HANDED_SPEC));
        assert!((phys_dmg_mod(&f) - 1.02).abs() < 1e-9);
        assert!(inc(&mut f, TWO_HANDED_SPEC));
        assert!((phys_dmg_mod(&f) - 1.03).abs() < 1e-9);
        assert!(!inc(&mut f, TWO_HANDED_SPEC), "maxed");
        f.equip(EquipmentSlot::Mainhand, SWORD);
        f.ctx().reevaluate_passives();
        assert!((phys_dmg_mod(&f) - 1.0).abs() < 1e-9);
        f.equip(EquipmentSlot::Mainhand, TWO_HAND_AXE);
        f.ctx().reevaluate_passives();
        assert!((phys_dmg_mod(&f) - 1.03).abs() < 1e-9);
        assert!(dec(&mut f, TWO_HANDED_SPEC));
        assert!((phys_dmg_mod(&f) - 1.02).abs() < 1e-9);
        f.ctx().reset();
        assert!(
            (phys_dmg_mod(&f) - 1.02).abs() < 1e-9,
            "ranks survive the reset"
        );
    }

    /// Port of `TestDefiance`: 5 % threat per rank (Forever), in Defensive Stance with a
    /// shield, nothing in Battle Stance.
    #[test]
    fn defiance_raises_threat_in_defensive_stance_only() {
        let mut f = fixture();
        f.equip(EquipmentSlot::Offhand, SHIELD);
        assert!(inc_n(&mut f, ANTICIPATION, 5));
        assert!(inc_n(&mut f, TOUGHNESS, 5));
        f.ctx().reset();
        f.engine.prepare_iteration(0.0);
        let defensive = f.spell_id(DEFENSIVE_STANCE);
        f.ctx().cast(defensive);
        assert_eq!(f.character.stance(), Stance::Defensive);
        let base = f.character.stats().get_total_threat_mod();
        assert!(
            (base - 1.3).abs() < 1e-9,
            "Defensive Stance passive: {base}"
        );
        for rank in 1..=3 {
            assert!(inc(&mut f, DEFIANCE));
            let threat = f.character.stats().get_total_threat_mod();
            let expected = 1.3 * (1.0 + 0.05 * f64::from(rank));
            assert!((threat - expected).abs() < 1e-9, "rank {rank}: {threat}");
        }
        assert_eq!(f.character.talents().unwrap().tab_points(PROTECTION), 13);

        f.engine.prepare_iteration(2.0);
        let battle = f.spell_id(BATTLE_STANCE);
        f.ctx().cast(battle);
        assert_eq!(f.character.stance(), Stance::Battle);
        let threat = f.character.stats().get_total_threat_mod();
        assert!((threat - 0.8).abs() < 1e-9, "Battle Stance: {threat}");
        f.ctx().clear_talent_tab(PROTECTION);
        assert!((f.character.stats().get_total_threat_mod() - 0.8).abs() < 1e-9);
    }

    /// Port of `TestArms::test_refilling_tree_after_switching_talent_setup` and
    /// `test_clearing_tree_after_filling`.
    #[test]
    fn setups_are_independent_and_switching_moves_the_effects() {
        let mut f = fixture();
        let base_crit = mh_crit(&f);
        spec_ms(&mut f);
        assert!(inc_n(&mut f, CRUELTY, 5));
        let points = |f: &Fixture| f.character.talents().unwrap().tab_points(ARMS);
        assert_eq!(points(&f), 31);
        assert_eq!(mh_crit(&f), base_crit + cruelty_crit(5));
        assert!(enabled(&f, MORTAL_STRIKE_R1));

        assert!(f.ctx().switch_talent_setup(1));
        assert_eq!(points(&f), 0);
        assert_eq!(mh_crit(&f), base_crit);
        assert!(!enabled(&f, MORTAL_STRIKE_R1));
        assert!(f.character.spell_modifiers().is_empty());
        spec_ms(&mut f);
        assert_eq!(points(&f), 31);
        assert!(enabled(&f, MORTAL_STRIKE_R1));
        assert_eq!(mh_crit(&f), base_crit, "Cruelty only in setup 0");

        assert!(f.ctx().switch_talent_setup(2));
        assert_eq!(points(&f), 0);
        spec_ms(&mut f);
        assert_eq!(points(&f), 31);
        assert!(!f.ctx().switch_talent_setup(2), "already current");
        assert!(!f.ctx().switch_talent_setup(7), "no such setup");

        assert!(f.ctx().switch_talent_setup(0));
        assert_eq!(points(&f), 31);
        assert_eq!(mh_crit(&f), base_crit + cruelty_crit(5));
        assert!(!dec(&mut f, TWO_HANDED_SPEC), "Mortal Strike holds tier 6");
        assert!(f.ctx().clear_talent_tab(ARMS));
        assert_eq!(points(&f), 0);
        assert_eq!(f.character.talents().unwrap().points_remaining(), 46);
        assert!(!enabled(&f, MORTAL_STRIKE_R1));
    }

    #[test]
    fn a_setup_is_spent_in_order_and_reports_what_it_could_not_reach() {
        let mut f = fixture();
        let short = f.ctx().spend_talent_points(&[
            (CRUELTY, 5),
            (UNBRIDLED_WRATH, 5),
            (DEATH_WISH, 1),
            (BLOODTHIRST_TALENT, 1),
        ]);
        assert_eq!(
            short,
            [(DEATH_WISH, 0), (BLOODTHIRST_TALENT, 0)],
            "tier 4 needs 20 points"
        );
        assert_eq!(rank(&f, CRUELTY), 5);
        assert_eq!(rank(&f, UNBRIDLED_WRATH), 5);
        // Blood Craze 3, Boundless Rage 3, Enrage 4 unlock tier 4 (20 points).
        let short = f.ctx().spend_talent_points(&[
            (CRUELTY, 5),
            (BLOOD_CRAZE, 3),
            (BOUNDLESS_RAGE, 3),
            (ENRAGE, 4),
            (DEATH_WISH, 1),
        ]);
        assert!(short.is_empty(), "{short:?}");
        assert_eq!(f.character.talents().unwrap().tab_points(FURY), 21);
        assert!(f.ctx().max_talent(ENRAGE));
        assert_eq!(rank(&f, ENRAGE), 5);
        assert!(!f.ctx().max_talent(ENRAGE), "nothing to add");
        assert!(
            !f.ctx().min_talent(CRUELTY),
            "tier 0 sits at exactly 5 with tier 1 invested"
        );
        assert_eq!(rank(&f, CRUELTY), 5);
        assert!(f.ctx().min_talent(DEATH_WISH));
        assert_eq!(rank(&f, DEATH_WISH), 0);
        assert!(f.ctx().min_talent(ENRAGE));
        assert_eq!(rank(&f, ENRAGE), 0);
        assert_eq!(f.character.talents().unwrap().tab_points(FURY), 16);
    }
}

// ---------------------------------------------------------------- rotation

/// Port of `Test/Rotation/TestRotationFileReader::test_warrior_dw_fury` (linking against a
/// real Warrior) and the builtin-variable cases of `TestConditionVariableBuiltin` that need a
/// character, plus an end-to-end run of a rotation through the event loop.
mod rotation {
    use super::*;
    use crate::attack_mode::AttackMode;
    use crate::rotation::condition::{BuiltinVariable, Comparator, Measure, Test};
    use crate::rotation::{ConditionContext, RotationDb, RotationSpec, Sentence};
    use crate::talent::{CharacterTalents, TalentDb};

    const BATTLE_SHOUT: u32 = 25289;

    /// An Orc Warrior with the shipped class, spell and talent data (no points spent), a
    /// sword and a dagger.
    pub(super) fn shipped_orc_warrior() -> Fixture {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let mut f = Fixture::orc_warrior();
        f.db = SpellDb::load(&data.join("spells")).expect("shipped spell data loads");
        let classes = super::super::ClassDb::load(&data.join("classes"), None).unwrap();
        f.character = Character::new(
            CharId(0),
            Arc::clone(classes.get(crate::faction::PlayerClass::Warrior).unwrap()),
            &race(Race::Orc),
            equipment_db(),
            Phase::MoltenCore,
            SimParams::default(),
            63,
            0,
            0,
        );
        let talents = TalentDb::load(&data.join("talents")).expect("shipped talent data loads");
        let tree = Arc::clone(talents.get(crate::faction::PlayerClass::Warrior).unwrap());
        f.ctx().set_talents(CharacterTalents::new(tree));
        f.equip(EquipmentSlot::Mainhand, SWORD);
        f.equip(EquipmentSlot::Offhand, DAGGER);
        let db = std::mem::take(&mut f.db);
        f.ctx().learn_all(&db);
        f.db = db;
        f
    }

    /// The shipped rotations (`data/rotations/`).
    fn rotations() -> RotationDb {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/rotations");
        RotationDb::load(&data).expect("shipped rotations load")
    }

    fn shipped(name: &str) -> Arc<RotationSpec> {
        Arc::clone(
            rotations()
                .get(crate::faction::PlayerClass::Warrior, name)
                .unwrap_or_else(|| panic!("no shipped rotation {name:?}")),
        )
    }

    fn dw_fury() -> Arc<RotationSpec> {
        shipped("DW Fury")
    }

    /// Covers every way an executor links or is skipped: an item use, talents not taken,
    /// another race's racial, conditions on buffs, resources, variables and cooldowns.
    const DW_FURY_LINKING: &str = r#"
class: WARRIOR
name: DW Fury (linking)
precombat_actions: [Bloodrage, Battle Shout, Berserker Stance]
cast_if:
  - name: Bloodrage
    condition: resource "Rage" less 70
  - name: Berserker Rage
    condition: resource "Rage" less 50
  - name: Battle Shout
    condition: |
      buff_duration "Battle Shout" less 3
      or variable "time_remaining_execute" less 10
      and variable "time_remaining_execute" greater 0
      and buff_duration "Battle Shout" less 45
  - name: Heroic Strike
    condition: |
      variable "time_remaining_execute" greater 3
      and resource "Rage" greater 50
  - name: Kiss of the Spider
    condition: buff_duration "Death Wish" is true
  - name: Death Wish
  - name: Elune's Light
    condition: buff_duration "Death Wish" is true
  - name: Blood Fury
  - name: Execute
  - name: Bloodthirst
  - name: Whirlwind
    condition: spell "Bloodthirst" greater 1.5
  - name: Berserker Stance
"#;

    #[test]
    fn a_rotation_links_the_executors_the_orc_warrior_has() {
        let mut f = shipped_orc_warrior();
        f.ctx()
            .set_rotation(Arc::new(serde_yaml::from_str(DW_FURY_LINKING).unwrap()));
        assert_eq!(f.character.rotation_name(), "DW Fury (linking)");
        assert_eq!(f.character.attack_mode(), AttackMode::MeleeAttack);
        let rotation = f.character.rotation().unwrap();

        let all: Vec<&str> = rotation
            .executors()
            .iter()
            .map(|e| e.spell_name())
            .collect();
        assert_eq!(
            all,
            [
                "Bloodrage",
                "Berserker Rage",
                "Battle Shout",
                "Heroic Strike",
                "Kiss of the Spider",
                "Death Wish",
                "Elune's Light",
                "Blood Fury",
                "Execute",
                "Bloodthirst",
                "Whirlwind",
                "Berserker Stance",
            ]
        );
        // No talents: Death Wish and Bloodthirst are not enabled; an Orc has no Elune's Light;
        // no trinkets are equipped.
        let active: Vec<&str> = rotation
            .active_executors()
            .map(|e| e.spell_name())
            .collect();
        assert_eq!(
            active,
            [
                "Bloodrage",
                "Berserker Rage",
                "Battle Shout",
                "Heroic Strike",
                "Blood Fury",
                "Execute",
                "Whirlwind",
                "Berserker Stance",
            ]
        );
        assert_eq!(
            rotation.precombat_spells(),
            [
                f.spell_id(BLOODRAGE),
                f.spell_id(BATTLE_SHOUT),
                f.spell_id(BERSERKER_STANCE)
            ]
        );
        assert_eq!(rotation.precast_spell(), None);

        // The skipped lines say why.
        let skipped: Vec<(usize, &str, String)> = rotation
            .skipped_executors()
            .map(|(line, e)| (line, e.spell_name(), e.skip_reason().unwrap().to_string()))
            .collect();
        let unknown = "no spell of this name";
        assert_eq!(
            skipped,
            [
                (5, "Kiss of the Spider", unknown.to_string()),
                (6, "Death Wish", "talent Death Wish not taken".to_string()),
                (7, "Elune's Light", unknown.to_string()),
                (
                    10,
                    "Bloodthirst",
                    "talent Bloodthirst not taken".to_string()
                ),
            ]
        );

        // Berserker Rage: one group of one resource sentence.
        let executors = rotation.executors();
        let groups = executors[1].linked().unwrap().condition.as_ref().unwrap();
        assert_eq!(groups.groups().len(), 1);
        assert_eq!(
            groups.groups()[0],
            [Sentence {
                measure: Measure::Resource(ResourceType::Rage),
                test: Test::Compare(Comparator::Less, 50.0),
            }]
        );
        // Battle Shout: two groups; the buff resolved to the party buff's handle.
        let battle_shout = f.spell_id(BATTLE_SHOUT);
        let groups = executors[2].linked().unwrap().condition.as_ref().unwrap();
        assert_eq!(executors[2].linked().unwrap().spell, battle_shout);
        assert_eq!(groups.groups().len(), 2);
        assert_eq!(groups.groups()[0].len(), 1);
        assert_eq!(groups.groups()[1].len(), 3);
        let shout_buff = f
            .character
            .spells()
            .buff_by_name("Battle Shout", 0, &f.raid, |_| true)
            .unwrap();
        assert_eq!(
            groups.groups()[0][0],
            Sentence {
                measure: Measure::BuffDuration(shout_buff),
                test: Test::Compare(Comparator::Less, 3.0),
            }
        );
        assert_eq!(
            groups.groups()[1][0].measure,
            Measure::Variable(BuiltinVariable::TimeRemainingExecute)
        );
        assert_eq!(
            groups.groups()[1][2].measure,
            Measure::BuffDuration(shout_buff)
        );
        // Heroic Strike: one group of two.
        let groups = executors[3].linked().unwrap().condition.as_ref().unwrap();
        let heroic_strike = executors[3].linked().unwrap().spell;
        let group = f.character.spells().rank_group("Heroic Strike").unwrap();
        assert_eq!(
            group.rank_of(heroic_strike),
            Some(group.max_rank()),
            "MAX_RANK"
        );
        assert_eq!(groups.groups().len(), 1);
        assert_eq!(
            groups.groups()[0],
            [
                Sentence {
                    measure: Measure::Variable(BuiltinVariable::TimeRemainingExecute),
                    test: Test::Compare(Comparator::Greater, 3.0),
                },
                Sentence {
                    measure: Measure::Resource(ResourceType::Rage),
                    test: Test::Compare(Comparator::Greater, 50.0),
                },
            ]
        );
        // Whirlwind's `spell "Bloodthirst"` resolved to the highest rank of the (disabled)
        // Bloodthirst.
        let groups = executors[10].linked().unwrap().condition.as_ref().unwrap();
        let Measure::SpellCooldown(bloodthirst) = groups.groups()[0][0].measure else {
            panic!("{:?}", groups.groups()[0][0]);
        };
        let group = f.character.spells().rank_group("Bloodthirst").unwrap();
        assert_eq!(group.rank_of(bloodthirst), Some(group.max_rank()));
        assert!(!f.character.spells().spell(bloodthirst).is_enabled());
        assert_eq!(
            executors[2].conditions_string(),
            "Battle Shout buff remaining < 3.0 seconds\n\
             OR\n\
             Time Remaining Until Execute < 10.0 seconds\n\
             Time Remaining Until Execute > 0.0 seconds\n\
             Battle Shout buff remaining < 45.0 seconds"
        );
    }

    #[test]
    fn relinking_picks_up_spells_enabled_later() {
        let mut f = shipped_orc_warrior();
        f.ctx().set_rotation(dw_fury());
        let is_active = |f: &Fixture, name: &str| {
            f.character
                .rotation()
                .unwrap()
                .active_executors()
                .any(|e| e.spell_name() == name)
        };
        assert!(!is_active(&f, "Bloodthirst"));
        // The executor asks for the highest learned rank.
        let bloodthirst = f
            .character
            .spells()
            .rank_group("Bloodthirst")
            .unwrap()
            .get_max_available_spell_rank(|_| true)
            .unwrap();
        assert_ne!(
            bloodthirst,
            f.spell_id(BLOODTHIRST),
            "rank 1 is not the highest"
        );
        f.ctx().enable_spell(bloodthirst);
        assert!(!is_active(&f, "Bloodthirst"), "not linked yet");
        f.ctx().prepare_set_of_combat_iterations();
        assert!(
            is_active(&f, "Bloodthirst"),
            "relinked before the iterations"
        );
        f.ctx().disable_spell(bloodthirst);
        f.ctx().relink_rotation();
        assert!(!is_active(&f, "Bloodthirst"));
        f.ctx().clear_rotation();
        assert!(f.character.rotation().is_none());
        assert_eq!(f.character.rotation_name(), "");
    }

    /// Port of `test_swing_timer_less` / `test_swing_timer_greater`: the builtin measures
    /// the time since the last main hand swing.
    #[test]
    fn time_since_swing_follows_the_mainhand_attack() {
        let mut f = shipped_orc_warrior();
        f.rig_rolls(PhysicalAttackResult::Hit);
        f.ctx().prepare_set_of_combat_iterations();
        f.engine.prepare_iteration(0.0);
        let less = |f: &mut Fixture, rhs: f64| {
            let ctx = f.ctx();
            Comparator::Less.holds(ctx.variable(BuiltinVariable::TimeSinceSwing), rhs)
        };
        assert!(less(&mut f, 0.2));
        assert!(less(&mut f, 0.3));
        f.ctx().start_attack();
        let iteration = f.character.spells().mh_attack().iteration();
        assert!(matches!(
            f.ctx().mh_swing(iteration),
            SwingOutcome::Swing(_)
        ));
        for (now, less_200, less_300) in [
            (0.1, true, true),
            (0.19, true, true),
            (0.21, false, true),
            (0.29, false, true),
            (0.31, false, false),
        ] {
            f.engine.prepare_iteration(now);
            assert_eq!(less(&mut f, 0.2), less_200, "{now}");
            assert_eq!(less(&mut f, 0.3), less_300, "{now}");
            // The time until the next swing is the rest of the 2.6 s sword speed.
            let ctx = f.ctx();
            let since = ctx.variable(BuiltinVariable::TimeSinceSwing);
            let remaining = ctx.variable(BuiltinVariable::TimeRemainingSwing);
            assert!((since - now).abs() < 1e-9, "{now}: since {since}");
            assert!(
                (since + remaining - 2.6).abs() < 1e-9,
                "{now}: remaining {remaining}"
            );
        }
    }

    #[test]
    fn builtin_variables_read_the_encounter_and_the_character() {
        let mut f = shipped_orc_warrior();
        f.character.set_sim(SimParams {
            combat_length: 200.0,
            execute_threshold: 0.2,
            ruleset: crate::rulesets::Ruleset::Standard,
        });
        f.engine
            .add_event(Event::new(50.0, EventKind::EncounterEnd));
        f.engine.next_event();
        f.character.gain_combo_points(1, f.engine.current_time());
        let ctx = f.ctx();
        let var = |v| ctx.variable(v);
        assert!((var(BuiltinVariable::TimeRemainingEncounter) - 150.0).abs() < 1e-9);
        assert!((var(BuiltinVariable::TimeRemainingExecute) - 110.0).abs() < 1e-9);
        assert!((var(BuiltinVariable::TargetHealth) - 0.75).abs() < 1e-9);
        assert_eq!(var(BuiltinVariable::ComboPoints), 1.0);
        assert_eq!(var(BuiltinVariable::TimeSinceAutoShot), 50.0);
        assert!(var(BuiltinVariable::MeleeAp) > 400.0);
        assert_eq!(var(BuiltinVariable::TimeRemainingGcd), 0.0);
        assert_eq!(ctx.time_required_to_run_precombat(), 1.5);
    }

    #[test]
    fn conditions_read_the_target_creature_type() {
        use crate::target::CreatureType;

        let mut f = shipped_orc_warrior();
        for creature in CreatureType::ALL {
            f.target.set_creature_type(creature);
            assert_eq!(f.ctx().target_creature_type(), creature);
        }
    }

    #[test]
    fn precombat_actions_run_before_the_pull() {
        let mut f = shipped_orc_warrior();
        f.ctx().set_rotation(dw_fury());
        f.ctx().prepare_set_of_combat_iterations();
        f.ctx().reset();
        f.engine.prepare_iteration(-1.5);
        f.ctx().run_precombat_actions();
        assert!(f.ctx().aura_active(BLOODRAGE_BUFF), "Bloodrage");
        assert!(f.ctx().aura_active(BATTLE_SHOUT), "Battle Shout");
        // Charge, the precast, needs Battle Stance; Berserker Stance waits for the pull.
        assert_eq!(f.character.stance(), Stance::Battle);
        // Precombat casts do not start the global cooldown (negative time); the stance swap
        // lag pushed it to -1.0.
        assert!(f.character.on_global_cooldown(-1.25));
        assert!(!f.character.on_global_cooldown(-1.0));
        // Charge's 1 s cast starts 1 s before the pull, so that it lands at the pull.
        assert!(!f.character.spells.cast_in_progress(), "Charge waits");
        f.run(-0.999);
        assert!(f.character.spells.cast_in_progress(), "Charge");
        // The player actions scheduled before the pull (the stance cooldown ending, the
        // Bloodrage rage) do not run the rotation.
        f.run(-0.001);
        assert!(
            f.character.spells.cast_in_progress(),
            "Charge lands at the pull"
        );
        assert_eq!(f.character.stance(), Stance::Battle);
        let stats = f.character.rotation().unwrap().statistics_by_spell();
        assert!(
            stats.values().all(|s| s.attempts() == 0),
            "no rotation before the pull: {stats:?}"
        );
    }

    /// A rotation whose casts do not depend on talents: Whirlwind whenever it is up, Heroic
    /// Strike above 50 rage, Bloodrage below 70, Battle Shout when it is about to fall off.
    pub(super) const FURY_NO_TALENTS: &str = r#"
class: WARRIOR
name: Fury (no talents)
precombat_actions: [Bloodrage, Battle Shout, Berserker Stance]
cast_if:
  - name: Bloodrage
    condition: resource "Rage" less 70
  - name: Battle Shout
    condition: buff_duration "Battle Shout" less 3
  - name: Heroic Strike
    condition: resource "Rage" greater 50
  - name: Blood Fury
  - name: Execute
  - name: Whirlwind
  - name: Battle Stance
    condition: variable "combo_points" greater 0
  - name: Berserker Stance
    condition: variable "combo_points" eq 0
"#;

    #[test]
    fn the_rotation_runs_through_the_event_loop() {
        let mut f = shipped_orc_warrior();
        f.rig_rolls(PhysicalAttackResult::Hit);
        f.ctx()
            .set_rotation(Arc::new(serde_yaml::from_str(FURY_NO_TALENTS).unwrap()));
        f.ctx().prepare_set_of_combat_iterations();
        f.ctx().reset();
        f.engine.prepare_iteration(-1.5);
        f.ctx().run_precombat_actions();
        f.engine.add_event(Event::new(
            0.0,
            EventKind::EncounterStart {
                character: CharId(0),
            },
        ));
        let handled = f.run(60.0);
        let actions = handled
            .iter()
            .filter(|kind| matches!(kind, EventKind::PlayerAction { .. }))
            .count();
        assert!(actions > 10, "{actions} player actions");

        let stats = f.character.rotation().unwrap().statistics_by_spell();
        let casts = |name: &str| stats[name].successful_casts;
        // Whirlwind has a 10 s cooldown: 6 casts in 60 s at most, and most of them.
        assert!(casts("Whirlwind") >= 4, "{:?}", stats["Whirlwind"]);
        assert!(casts("Whirlwind") <= 6, "{:?}", stats["Whirlwind"]);
        assert!(stats["Whirlwind"].spell_status[&SpellStatus::OnCooldown] > 0);
        // Whirlwind spends the rage first; Heroic Strike is tried every action all the same.
        assert!(
            stats["Heroic Strike"].attempts() > 10,
            "{:?}",
            stats["Heroic Strike"]
        );
        assert!(casts("Bloodrage") >= 1, "{:?}", stats["Bloodrage"]);
        assert!(casts("Blood Fury") >= 1, "{:?}", stats["Blood Fury"]);
        assert_eq!(
            casts("Battle Shout"),
            0,
            "the precombat shout lasts 2 minutes"
        );
        assert!(stats["Battle Shout"].no_condition_group_fulfilled > 0);
        assert_eq!(casts("Execute"), 0, "not in execute range");
        assert!(
            stats["Execute"].spell_status[&SpellStatus::NotInExecuteRange] > 0,
            "{:?}",
            stats["Execute"]
        );
        // Every roll hits, so Overpower's combo point never comes: Berserker Stance stays,
        // and re-casting it is refused as "already in it".
        assert_eq!(casts("Battle Stance"), 0);
        assert!(stats["Battle Stance"].no_condition_group_fulfilled > 0);
        assert_eq!(casts("Berserker Stance"), 0);
        assert!(stats["Berserker Stance"].spell_status[&SpellStatus::InBerserkerStance] > 0);
        assert_eq!(f.character.stance(), Stance::Berserker);

        // Zeroed for the next set of iterations.
        f.ctx().prepare_set_of_combat_iterations();
        let stats = f.character.rotation().unwrap().statistics_by_spell();
        assert_eq!(stats["Whirlwind"].attempts(), 0);
    }

    /// A queued Heroic Strike is a buff of its name: `is false` queues it once per swing
    /// instead of re-queueing it on every action.
    #[test]
    fn a_rotation_skips_a_queued_heroic_strike() {
        let run = |condition: &str| {
            let rotation = format!(
                "class: WARRIOR\nname: HS\ncast_if:\n  - name: Bloodrage\n  \
                 - name: Heroic Strike\n    condition: |\n      {condition}\n"
            );
            let mut f = shipped_orc_warrior();
            f.rig_rolls(PhysicalAttackResult::Hit);
            f.ctx()
                .set_rotation(Arc::new(serde_yaml::from_str(&rotation).unwrap()));
            let marker =
                crate::rotation::executor::RotationHost::buff_by_name(&f.ctx(), "Heroic Strike")
                    .unwrap();
            let hs = f.character.rotation().unwrap().executors()[1]
                .linked()
                .unwrap()
                .spell;
            assert_eq!(f.character.spells().spell(hs).marker_buff(), Some(marker));
            f.ctx().prepare_set_of_combat_iterations();
            f.ctx().reset();
            f.engine.prepare_iteration(0.0);
            f.set_rage(100);
            f.engine.add_event(Event::new(
                0.0,
                EventKind::EncounterStart {
                    character: CharId(0),
                },
            ));
            let handled = f.run(30.0);
            let swings = handled
                .iter()
                .filter(|kind| matches!(kind, EventKind::MainhandMeleeHit { .. }))
                .count() as u64;
            let stats = f.character.rotation().unwrap().statistics_by_spell();
            (stats["Heroic Strike"].successful_casts, swings)
        };
        let (requeued, swings) = run("resource \"Rage\" greater 15");
        assert!(requeued > swings, "{requeued} casts for {swings} swings");
        let (queued_once, swings) =
            run("resource \"Rage\" greater 15\n      and buff_duration \"Heroic Strike\" is false");
        assert!(queued_once > 0);
        assert!(
            queued_once <= swings + 1,
            "{queued_once} casts for {swings} swings"
        );
    }
}

mod statistics {
    use super::rotation::{FURY_NO_TALENTS, shipped_orc_warrior};
    use super::*;
    use crate::statistics::{ClassStatistics, Outcome, SpellStatistics};

    const IMPROVED_REND: u32 = 105956;
    const DEFLECTION: u32 = 105957;
    const IMPROVED_TACTICAL_MASTERY: u32 = 105954;
    const IMPROVED_OVERPOWER: u32 = 105952;
    const ANGER_MANAGEMENT: u32 = 105951;
    const DEEP_WOUNDS: u32 = 105950;
    const CRUELTY: u32 = 105939;
    const UNBRIDLED_WRATH: u32 = 105937;
    const UNBRIDLED_WRATH_SPELL: u32 = 12322;

    /// The Orc with the no-talent Fury rotation and every roll a hit, ready to pull at 0.
    fn ready_to_pull(f: &mut Fixture) {
        ready_to_pull_with(f, FURY_NO_TALENTS);
    }

    /// As `ready_to_pull`, with the rotation given as YAML.
    fn ready_to_pull_with(f: &mut Fixture, rotation: &str) {
        f.rig_rolls(PhysicalAttackResult::Hit);
        f.ctx()
            .set_rotation(Arc::new(serde_yaml::from_str(rotation).unwrap()));
        f.ctx().prepare_set_of_combat_iterations();
        f.ctx().reset();
        f.engine.prepare_iteration(-1.5);
        f.ctx().run_precombat_actions();
        f.engine.add_event(Event::new(
            0.0,
            EventKind::EncounterStart {
                character: CharId(0),
            },
        ));
    }

    /// The statistics of the spell called `name`, whatever its rank.
    fn spell<'a>(stats: &'a ClassStatistics, name: &str) -> &'a SpellStatistics {
        stats
            .spells()
            .find(|(key, _)| key.name == name)
            .unwrap_or_else(|| panic!("no statistics for {name}"))
            .1
    }

    #[test]
    fn a_fight_records_spells_swings_ticks_buffs_and_resources() {
        let mut f = shipped_orc_warrior();
        ready_to_pull(&mut f);
        let handled = f.run(60.0);
        let events =
            |kind: fn(&EventKind) -> bool| handled.iter().filter(|k| kind(k)).count() as u64;
        let mh_events = events(|k| matches!(k, EventKind::MainhandMeleeHit { .. }));
        let oh_events = events(|k| matches!(k, EventKind::OffhandMeleeHit { .. }));
        let ticks = events(|k| matches!(k, EventKind::DotTick { .. }));
        f.ctx().reset();
        f.character.finish_combat_iteration();
        let stats = f.ctx().take_statistics();

        // Every main hand swing event landed a white swing or the queued Heroic Strike
        // (Whirlwind spends the rage first, so the Heroic Strike may never come).
        let mh = spell(&stats, "Mainhand Attack");
        let hs_attempts: u64 = stats
            .spells()
            .filter(|(key, _)| key.name == "Heroic Strike")
            .map(|(_, s)| s.total_attempts())
            .sum();
        assert!(mh.total_attempts() > 10, "{mh:?}");
        assert_eq!(mh.total_attempts() + hs_attempts, mh_events);
        assert_eq!(
            mh.hits(),
            mh.total_attempts(),
            "every roll is rigged to hit"
        );
        assert_eq!(mh.attempts(Outcome::Crit), 0);
        assert!(mh.damage(Outcome::Hit).min() >= 80, "{mh:?}");
        assert!(mh.damage(Outcome::Hit).max() > mh.damage(Outcome::Hit).min());
        assert!(
            mh.total_threat() < mh.total_damage(),
            "Berserker Stance lowers threat"
        );
        assert!(!mh.dpr().is_set(), "white swings cost nothing");
        let oh = spell(&stats, "Offhand Attack");
        assert_eq!(oh.total_attempts(), oh_events);
        assert!(oh.damage(Outcome::Hit).max() < mh.damage(Outcome::Hit).min());

        // Whirlwind: one recorded attempt per successful executor cast, GCD execution time.
        let ww = spell(&stats, "Whirlwind");
        let executors = stats.executors();
        let ww_executor = executors
            .iter()
            .find(|e| e.spell_name() == "Whirlwind")
            .unwrap();
        assert!(
            ww_executor.name().starts_with('('),
            "{}",
            ww_executor.name()
        );
        assert_eq!(ww.total_attempts(), ww_executor.successful_casts());
        assert!(ww.total_attempts() >= 4, "{ww:?}");
        assert!(ww.dpet().is_set());
        assert!((ww.dpet().avg() - ww.damage(Outcome::Hit).avg() / 1.5).abs() < 1e-6);
        assert_eq!(executors.len(), 8, "the active executors, in file order");
        assert_eq!(executors[0].spell_name(), "Bloodrage");
        assert!(executors[0].outcomes().len() >= 2);

        // Bloodrage: 10 rage up front and 1 per tick, no damage.
        let bloodrage = stats.resource_statistics("Bloodrage", 1).unwrap();
        let bloodrage_casts = executors[0].successful_casts() + 1;
        assert!(
            bloodrage.gain(ResourceType::Rage) >= (bloodrage_casts * 10) as f64,
            "{bloodrage:?} for {bloodrage_casts} casts"
        );
        assert!(ticks > 0);
        assert!(
            stats.spell_statistics("Bloodrage", 1).is_none(),
            "no damage"
        );
        // The swings generate rage.
        let mh_rage = stats.resource_statistics("Mainhand Attack", 1).unwrap();
        assert!(mh_rage.gain(ResourceType::Rage) > 0.0);
        assert!(mh_rage.gain_per_5(ResourceType::Rage, stats.time_in_combat()) > 0.0);

        // Buffs: the precombat Battle Shout ran from -1.5 s to the reset at 60 s.
        let shout = stats.buff_statistics("Battle Shout (party 1)").unwrap();
        assert!(!shout.is_debuff());
        assert!((shout.max_uptime() - 61.5).abs() < 1e-9, "{shout:?}");
        assert!((shout.min_uptime() - 61.5).abs() < 1e-9, "{shout:?}");
        assert_eq!(shout.encounters(), 1);
        assert!(
            (shout.avg_uptime() - 61.5 / 300.0).abs() < 1e-9,
            "{shout:?}"
        );
        // Blood Fury expired on its own.
        let blood_fury = stats.buff_statistics("Blood Fury").unwrap();
        assert!(blood_fury.max_uptime() > 0.0 && blood_fury.max_uptime() < 60.0);
        assert!(stats.buffs().all(|b| b.encounters() == 1));
        // Heroic Strike's queue marker: up from each queue to the swing that takes or drops it.
        let queued = stats.buff_statistics("Heroic Strike").unwrap();
        assert!(!queued.is_debuff());
        assert!(
            queued.max_uptime() > 0.0 && queued.max_uptime() < 60.0,
            "{queued:?}"
        );

        // Totals and the iteration.
        assert_eq!(
            stats.total_damage(),
            stats.spells().map(|(_, s)| s.total_damage()).sum::<u64>()
        );
        assert_eq!(stats.iterations(), 1);
        assert_eq!(stats.time_in_combat(), 300.0);
        assert!((stats.personal_dps() - stats.total_damage() as f64 / 300.0).abs() < 1e-9);
        assert_eq!(stats.dps_per_iteration(), &[stats.personal_dps()]);
        assert_eq!(stats.personal_result().player_name, "You");
        // The engine's counters came along.
        assert_eq!(
            stats
                .engine()
                .event_count(crate::engine::EventType::MainhandMeleeHit),
            mh_events
        );
        assert!(stats.engine().total_events() > mh_events + oh_events);

        // Taking left fresh statistics behind.
        assert_eq!(f.character.statistics().spells().count(), 0);
        assert_eq!(f.character.statistics().iterations(), 0);
        assert_eq!(f.character.statistics().combat_length(), 300.0);
    }

    /// Deep Wounds procs on every crit and its bleed ticks under the bleed's name; Anger
    /// Management's periodic rage is a resource source.
    #[test]
    fn procs_their_payloads_and_periodic_resources_are_recorded() {
        let mut f = shipped_orc_warrior();
        let short = f.ctx().spend_talent_points(&[
            (IMPROVED_REND, 3),
            (DEFLECTION, 2),
            (IMPROVED_TACTICAL_MASTERY, 5),
            (IMPROVED_OVERPOWER, 2),
            (ANGER_MANAGEMENT, 1),
            (DEEP_WOUNDS, 3),
        ]);
        assert!(short.is_empty(), "{short:?}");
        f.character.stats_mut().increase_melee_base_crit(10000);
        ready_to_pull(&mut f);
        f.run(30.0);
        f.ctx().sync_statistics();
        let stats = f.character.statistics();

        let mh = spell(stats, "Mainhand Attack");
        assert_eq!(mh.crits(), mh.total_attempts(), "100 % crit");
        let crits: u64 = stats.spells().map(|(_, s)| s.crits()).sum();
        let deep_wounds = stats.proc_statistics("Deep Wounds").unwrap();
        assert_eq!(deep_wounds.attempts(), crits, "one attempt per crit");
        assert_eq!(deep_wounds.procs(), crits, "always procs");
        assert_eq!(deep_wounds.avg_proc_rate(), 1.0);
        assert!(
            stats.procs().all(|p| p.name() == "Deep Wounds"),
            "only the enabled proc is listed: {:?}",
            stats.procs().map(|p| p.name()).collect::<Vec<_>>()
        );
        // The bleed ticks every 3 s for 12 s and refreshes: at most 10 ticks in 30 s.
        let bleed = spell(stats, "Deep Wounds");
        assert!(bleed.hits() >= 8 && bleed.hits() <= 10, "{bleed:?}");
        assert_eq!(bleed.hits(), bleed.total_attempts());
        assert!(bleed.total_damage() > 0);
        assert!(bleed.damage(Outcome::Hit).min() > 0);
        assert!(!bleed.dpr().is_set(), "the bleed costs nothing");
        // Anger Management: 1 rage every 3 s.
        let anger = stats.resource_statistics("Anger Management", 1).unwrap();
        assert!(anger.gain(ResourceType::Rage) >= 9.0, "{anger:?}");
        assert!(anger.gain(ResourceType::Rage) <= 11.0, "{anger:?}");
        assert!(stats.spell_statistics("Anger Management", 1).is_none());

        // Syncing again does not double the counts.
        f.ctx().sync_statistics();
        assert_eq!(
            f.character
                .statistics()
                .proc_statistics("Deep Wounds")
                .unwrap()
                .procs(),
            crits
        );
    }

    /// Unbridled Wrath rolls on every landed swing of either hand (not on the abilities) and
    /// the rage it gives is a resource source under the talent's name.
    #[test]
    fn unbridled_wrath_procs_off_landed_swings_and_its_rage_is_recorded() {
        let mut f = shipped_orc_warrior();
        let short = f
            .ctx()
            .spend_talent_points(&[(CRUELTY, 5), (UNBRIDLED_WRATH, 5)]);
        assert!(short.is_empty(), "{short:?}");
        let proc = f
            .character
            .spells()
            .proc_by_game_id(UNBRIDLED_WRATH_SPELL)
            .unwrap();
        assert!(f.character.spells().procs().is_enabled(proc));
        let rank = f.character.spells().procs().get(proc).spell().rank();
        ready_to_pull(&mut f);
        f.run(60.0);
        f.ctx().sync_statistics();
        let stats = f.character.statistics();

        let mh = spell(stats, "Mainhand Attack");
        let oh = spell(stats, "Offhand Attack");
        let unbridled_wrath = stats.proc_statistics("Unbridled Wrath").unwrap();
        assert_eq!(
            unbridled_wrath.attempts(),
            mh.hits() + oh.hits(),
            "one roll per landed swing of either hand"
        );
        assert!(unbridled_wrath.attempts() > 20, "{unbridled_wrath:?}");
        assert!(
            unbridled_wrath.procs() > 0 && unbridled_wrath.procs() < unbridled_wrath.attempts(),
            "60 %: {unbridled_wrath:?}"
        );
        // One rage per proc with a one-hander, less when the rage bar was full.
        let rage = stats
            .resource_statistics("Unbridled Wrath", rank)
            .expect("the proc's rage is a resource source");
        assert!(rage.gain(ResourceType::Rage) > 0.0, "{rage:?}");
        assert!(
            rage.gain(ResourceType::Rage) <= unbridled_wrath.procs() as f64,
            "{rage:?} for {unbridled_wrath:?}"
        );
        assert!(
            stats.spell_statistics("Unbridled Wrath", rank).is_none(),
            "no damage"
        );
    }

    /// A landed Heroic Strike replaces the white swing and does not roll Unbridled Wrath.
    #[test]
    fn unbridled_wrath_does_not_proc_off_heroic_strike() {
        const HEROIC_STRIKE_ONLY: &str = r#"
class: WARRIOR
name: Heroic Strike only
precombat_actions: [Bloodrage, Berserker Stance]
cast_if:
  - name: Bloodrage
  - name: Heroic Strike
"#;
        let mut f = shipped_orc_warrior();
        let short = f
            .ctx()
            .spend_talent_points(&[(CRUELTY, 5), (UNBRIDLED_WRATH, 5)]);
        assert!(short.is_empty(), "{short:?}");
        ready_to_pull_with(&mut f, HEROIC_STRIKE_ONLY);
        f.run(60.0);
        f.ctx().sync_statistics();
        let stats = f.character.statistics();

        let hs_hits: u64 = stats
            .spells()
            .filter(|(key, _)| key.name == "Heroic Strike")
            .map(|(_, s)| s.hits())
            .sum();
        assert!(hs_hits > 10, "Heroic Strike lands: {hs_hits}");
        let mh = spell(stats, "Mainhand Attack");
        let oh = spell(stats, "Offhand Attack");
        let unbridled_wrath = stats.proc_statistics("Unbridled Wrath").unwrap();
        assert_eq!(
            unbridled_wrath.attempts(),
            mh.hits() + oh.hits(),
            "white swings only, not the {hs_hits} Heroic Strikes"
        );
    }

    #[test]
    fn a_new_set_of_iterations_starts_from_empty_statistics() {
        let mut f = shipped_orc_warrior();
        ready_to_pull(&mut f);
        f.run(20.0);
        f.ctx().reset();
        f.character.finish_combat_iteration();
        assert!(f.character.statistics().total_damage() > 0);
        assert_eq!(f.character.statistics().iterations(), 1);

        f.ctx().prepare_set_of_combat_iterations();
        let stats = f.character.statistics();
        assert_eq!(stats.total_damage(), 0);
        assert_eq!(stats.iterations(), 0);
        assert_eq!(stats.spells().count(), 0);
        assert_eq!(stats.buffs().count(), 0);
    }
}

mod rulesets {
    use super::rotation::shipped_orc_warrior;
    use super::*;
    use crate::rulesets::{ESSENCE_OF_THE_RED, Ruleset};
    use crate::stats::TargetStatView;

    fn mh_crit(f: &Fixture) -> u32 {
        let target = TargetStatView::default();
        let ctx = f.character.stat_context(&target);
        f.character.stats().get_mh_crit_chance(&ctx)
    }

    fn set_ruleset(f: &mut Fixture, ruleset: Ruleset) {
        let db = std::mem::take(&mut f.db);
        f.ctx().set_sim(
            SimParams {
                ruleset,
                ..SimParams::default()
            },
            &db,
        );
        f.db = db;
    }

    #[test]
    fn loatheb_adds_melee_crit_until_the_ruleset_changes() {
        let mut f = shipped_orc_warrior();
        let base = mh_crit(&f);
        set_ruleset(&mut f, Ruleset::Loatheb);
        // +100 % aura crit, less the aura crit suppression against a level 63 target: every
        // swing that is not avoided crits.
        let loatheb = mh_crit(&f);
        assert!(loatheb > 10_000, "{base} -> {loatheb}");
        assert!(
            !f.character
                .roll_context(&TargetStatView::default())
                .glancing_blows
        );
        set_ruleset(&mut f, Ruleset::Loatheb);
        assert_eq!(mh_crit(&f), loatheb, "applied once");
        set_ruleset(&mut f, Ruleset::Standard);
        assert_eq!(mh_crit(&f), base);
        assert!(
            f.character
                .roll_context(&TargetStatView::default())
                .glancing_blows
        );
    }

    #[test]
    fn standard_learns_no_ruleset_spell() {
        let mut f = shipped_orc_warrior();
        set_ruleset(&mut f, Ruleset::Standard);
        assert!(!f.character.spells().has_game_id(ESSENCE_OF_THE_RED));
    }

    #[test]
    fn vaelastrasz_gives_twenty_rage_per_second_from_the_pull() {
        let mut f = shipped_orc_warrior();
        set_ruleset(&mut f, Ruleset::Vaelastrasz);
        let essence = f.spell_id(ESSENCE_OF_THE_RED);
        assert!(f.character.spells().spell(essence).is_enabled());
        assert!(
            f.character
                .spells()
                .start_of_combat_spells()
                .contains(&essence)
        );

        f.set_rage(0);
        f.ctx().encounter_start();
        assert!(f.ctx().aura_active(ESSENCE_OF_THE_RED));
        // Keep the auto attacks and their rage out of the count.
        f.character.spells_mut().stop_attack();
        f.advance_to(3.5);
        assert_eq!(f.rage(), 60, "three ticks of 20 rage");
    }

    #[test]
    fn leaving_vaelastrasz_disables_essence_of_the_red() {
        let mut f = shipped_orc_warrior();
        set_ruleset(&mut f, Ruleset::Vaelastrasz);
        set_ruleset(&mut f, Ruleset::Loatheb);
        let essence = f.spell_id(ESSENCE_OF_THE_RED);
        assert!(!f.character.spells().spell(essence).is_enabled());
        f.set_rage(0);
        f.ctx().encounter_start();
        assert!(!f.ctx().aura_active(ESSENCE_OF_THE_RED));
    }
}
