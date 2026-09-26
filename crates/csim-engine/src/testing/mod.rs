//! Test harness for spell, buff, proc and talent tests: a level-60 character built from the
//! shipped `data/` bundle plus a few synthetic test items, with helpers to equip weapons, force
//! attack-table outcomes, spend talent points, step the engine and assert on the statistics.
//! Port of `Test/TestSpell` (the given / when / then vocabulary is kept); the Warrior layer,
//! `TestSpellWarrior`, is [`warrior::WarriorTest`].

// The vocabulary is complete up front; the ported C++ tests pick from it.
#![allow(dead_code)]

pub(crate) mod warrior;

#[cfg(test)]
mod equipment_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod warrior_tests;

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use crate::attack_mode::AttackMode;
use crate::buff::Buff;
use crate::character::Character;
use crate::character::context::CharacterContext;
use crate::character_loader::{MAX_LEVEL, MAX_TARGET_LEVEL};
use crate::combat_roll::{IncludedOutcomes, PhysicalAttackResult, ROLL_RANGE};
use crate::data_bundle::DataBundle;
use crate::effect::EffectHost;
use crate::engine::{Event, EventType};
use crate::faction::PlayerClass;
use crate::ids::{BuffId, CharId, ProcId, SpellId};
use crate::item::{EquipmentSlot, Item, ItemSpec, WeaponType};
use crate::proc::ProcSource;
use crate::race::Race;
use crate::raid::{RaidControl, SharedBuffRegistry};
use crate::resource::ResourceType;
use crate::rng::Random;
use crate::rotation::RotationHost;
use crate::sim_settings::SimSettings;
use crate::spell::{CastReport, Hand, MAX_RANK, SpellHost, SpellStatus, SwingReport};
use crate::stats::{CharacterStats, StatContext, TargetStatView};
use crate::talent::CharacterTalents;
use crate::target::{CreatureType, Target};

/// Pass as `act` to [`SpellTest::then_next_event_is`] to also dispatch the event.
pub(crate) const RUN_EVENT: bool = true;

/// Seed of every harness character, so a test that does roll is reproducible.
const SEED: u64 = 42;

/// Ids of the synthetic test items (`TestUtils::Weapons`).
pub(crate) mod items {
    /// One-hand sword, 100 - 100 damage, 2.6 speed.
    pub const TEST_100_DMG: u32 = 1_000_000;
    /// Dagger, 100 - 100 damage, 2.0 speed.
    pub const TEST_100_DMG_DAGGER: u32 = 1_000_001;
    /// One-hand sword, 100 - 100 damage, 3.0 speed.
    pub const TEST_3_SPEED: u32 = 1_000_002;
    /// One-hand sword, 100 - 100 damage, 2.0 speed.
    pub const TEST_2_SPEED: u32 = 1_000_003;
    /// Two-hand sword, 100 - 100 damage, 3.5 speed.
    pub const TEST_100_DMG_2H: u32 = 1_000_004;
    /// One-hand axe, 100 - 100 damage, 2.0 speed.
    pub const TEST_AXE: u32 = 1_000_005;
    /// Ring with +5 sword skill.
    pub const TEST_5_SWORD_SKILL: u32 = 1_000_006;
    /// Ring with +10 sword skill.
    pub const TEST_10_SWORD_SKILL: u32 = 1_000_007;
    /// Ring with +15 sword skill.
    pub const TEST_15_SWORD_SKILL: u32 = 1_000_008;
    /// Bow, 100 - 100 damage, 2.6 speed.
    pub const TEST_100_DMG_RANGED: u32 = 1_000_009;
    /// Bow, 100 - 100 damage, 3.0 speed.
    pub const TEST_3_SPEED_RANGED: u32 = 1_000_010;
    /// Bow, 100 - 100 damage, 2.0 speed.
    pub const TEST_2_SPEED_RANGED: u32 = 1_000_011;
}

const TEST_ITEMS_YAML: &str = r#"
- { id: 1000000, name: Test 100 dmg, phase: 1, slot: 1H, type: SWORD, quality: EPIC,
    damage: { min: 100, max: 100, speed: 2.6 } }
- { id: 1000001, name: Test 100 dmg Dagger, phase: 1, slot: 1H, type: DAGGER, quality: EPIC,
    damage: { min: 100, max: 100, speed: 2.0 } }
- { id: 1000002, name: Test 3 Speed, phase: 1, slot: 1H, type: SWORD, quality: EPIC,
    damage: { min: 100, max: 100, speed: 3.0 } }
- { id: 1000003, name: Test 2 Speed, phase: 1, slot: 1H, type: SWORD, quality: EPIC,
    damage: { min: 100, max: 100, speed: 2.0 } }
- { id: 1000004, name: Test 100 dmg 2h, phase: 1, slot: 2H, type: TWOHAND_SWORD, quality: EPIC,
    damage: { min: 100, max: 100, speed: 3.5 } }
- { id: 1000005, name: Test Axe, phase: 1, slot: 1H, type: AXE, quality: EPIC,
    damage: { min: 100, max: 100, speed: 2.0 } }
- { id: 1000006, name: Test +5 Sword Skill, phase: 1, slot: RING, type: RING, quality: EPIC,
    stats: { SWORD_SKILL: 5 } }
- { id: 1000007, name: Test +10 Sword Skill, phase: 1, slot: RING, type: RING, quality: EPIC,
    stats: { SWORD_SKILL: 10 } }
- { id: 1000008, name: Test +15 Sword Skill, phase: 1, slot: RING, type: RING, quality: EPIC,
    stats: { SWORD_SKILL: 15 } }
- { id: 1000009, name: Test 100 dmg Ranged, phase: 1, slot: RANGED, type: BOW, quality: EPIC,
    damage: { min: 100, max: 100, speed: 2.6 } }
- { id: 1000010, name: Test 3 Speed Ranged, phase: 1, slot: RANGED, type: BOW, quality: EPIC,
    damage: { min: 100, max: 100, speed: 3.0 } }
- { id: 1000011, name: Test 2 Speed Ranged, phase: 1, slot: RANGED, type: BOW, quality: EPIC,
    damage: { min: 100, max: 100, speed: 2.0 } }
"#;

/// The repository's `data/` bundle with the [`items`] added, loaded once per test binary.
pub(crate) fn data() -> &'static DataBundle {
    static DATA: OnceLock<DataBundle> = OnceLock::new();
    DATA.get_or_init(|| {
        let mut data =
            DataBundle::load(&DataBundle::repository_dir()).expect("the data/ bundle loads");
        let mut equipment = (*data.equipment).clone();
        let specs: Vec<ItemSpec> = serde_yaml::from_str(TEST_ITEMS_YAML).unwrap();
        for spec in specs {
            equipment.add_item(Item::from_spec(spec).unwrap()).unwrap();
        }
        data.equipment = Arc::new(equipment);
        data
    })
}

/// An attack-table outcome a test can force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Hit,
    Crit,
    Glancing,
    Miss,
    Dodge,
    Parry,
    Block,
}

impl Outcome {
    fn result(self) -> PhysicalAttackResult {
        match self {
            Outcome::Hit => PhysicalAttackResult::Hit,
            Outcome::Crit => PhysicalAttackResult::Critical,
            Outcome::Glancing => PhysicalAttackResult::Glancing,
            Outcome::Miss => PhysicalAttackResult::Miss,
            Outcome::Dodge => PhysicalAttackResult::Dodge,
            Outcome::Parry => PhysicalAttackResult::Parry,
            Outcome::Block => PhysicalAttackResult::Block,
        }
    }

    fn chance(self, outcome: Outcome) -> f64 {
        if self == outcome { 1.0 } else { 0.0 }
    }
}

/// One character in a raid of its own, facing a level 63 target. Port of `TestSpell`.
pub(crate) struct SpellTest {
    pub raid: RaidControl,
    pub id: CharId,
    /// What is under test, for the assertion messages (`spell_under_test`).
    label: String,
    ignored_events: HashSet<EventType>,
    /// The crit adjustment of the last forced outcome, undone by the next one.
    forced_crit: Option<Outcome>,
}

impl SpellTest {
    /// A level 60 character of `class` and `race` with every spell of its class learned, an
    /// empty talent setup, no gear and no rotation, in a raid at T = 0 whose set of iterations
    /// is not prepared yet (see [`Self::prepare_set_of_combat_iterations`]).
    pub fn new(class: PlayerClass, race: Race, label: &str) -> Self {
        let mut raid = RaidControl::new(Target::new(MAX_TARGET_LEVEL));
        let id = Self::add_character_to(&mut raid, class, race, None);
        raid.set_seed(SEED);
        SpellTest {
            raid,
            id,
            label: label.to_string(),
            ignored_events: HashSet::new(),
            forced_crit: None,
        }
    }

    /// Adds another level 60 character to the raid, at `place` (party, member) or the first
    /// free place, set up like the one under test.
    pub fn add_character(
        &mut self,
        class: PlayerClass,
        race: Race,
        place: Option<(u8, u8)>,
    ) -> CharId {
        let id = Self::add_character_to(&mut self.raid, class, race, place);
        self.raid.set_seed(SEED);
        id
    }

    fn add_character_to(
        raid: &mut RaidControl,
        class: PlayerClass,
        race: Race,
        place: Option<(u8, u8)>,
    ) -> CharId {
        let data = data();
        let settings = SimSettings::default();
        let class_spec = Arc::clone(data.classes.get(class).expect("the class exists"));
        let race_spec = data.races.get(race);
        let build = |id, party, member| {
            Character::new(
                id,
                class_spec,
                race_spec,
                Arc::clone(&data.equipment),
                settings.phase,
                settings.sim_params(),
                MAX_TARGET_LEVEL,
                party,
                member,
            )
        };
        let id = match place {
            Some((party, member)) => raid.add_character_at(party, member, build),
            None => raid.add_character(build),
        }
        .expect("a free place");
        raid.character_mut(id).set_clvl(MAX_LEVEL);
        raid.with_character(id, |ctx| {
            if let Some(file) = data.talents.get(class) {
                ctx.set_talents(CharacterTalents::new(Arc::clone(file)));
            }
            ctx.learn_all(&data.spells);
            ctx.sync_ruleset_spells(&data.spells);
        });
        id
    }

    /// Prepares the raid (and so the character) for a set of iterations.
    pub fn prepare_set_of_combat_iterations(&mut self) {
        self.raid.prepare_set_of_combat_iterations();
    }

    // ---------------------------------------------------------------- access

    pub fn character(&self) -> &Character {
        self.raid.character(self.id)
    }

    pub fn character_mut(&mut self) -> &mut Character {
        self.raid.character_mut(self.id)
    }

    /// Runs `f` with the character's context, then propagates party aura changes.
    pub fn with_ctx<R>(
        &mut self,
        f: impl FnOnce(&mut CharacterContext<'_, SharedBuffRegistry>) -> R,
    ) -> R {
        self.raid.with_character(self.id, f)
    }

    pub fn now(&self) -> f64 {
        self.raid.engine().current_time()
    }

    pub fn target(&self) -> &Target {
        self.raid.target()
    }

    pub fn target_mut(&mut self) -> &mut Target {
        self.raid.target_mut()
    }

    fn view(&self) -> TargetStatView {
        self.raid.target().stat_view()
    }

    /// Reads the character's stats with their context.
    pub fn stat<R>(&self, f: impl FnOnce(&CharacterStats, &StatContext) -> R) -> R {
        let view = self.view();
        let character = self.character();
        f(character.stats(), &character.stat_context(&view))
    }

    pub fn stats_mut(&mut self) -> &mut CharacterStats {
        self.character_mut().stats_mut()
    }

    // ---------------------------------------------------------------- lookups

    /// The highest learned rank of spell `name`.
    pub fn spell(&mut self, name: &str) -> SpellId {
        self.with_ctx(|ctx| ctx.spell_by_name(name, MAX_RANK))
            .unwrap_or_else(|| panic!("{}: no learned spell {name:?}", self.label))
    }

    /// The buff `name` (owned, or the character's handle of a shared one).
    pub fn buff(&mut self, name: &str) -> BuffId {
        self.with_ctx(|ctx| ctx.buff_by_name(name))
            .unwrap_or_else(|| panic!("{}: no buff {name:?}", self.label))
    }

    /// The buff of game spell `spell`, for names several buffs share (the Flurry talent's
    /// passive and its haste buff).
    pub fn buff_by_spell(&mut self, spell: u32) -> BuffId {
        let ids: Vec<BuffId> = self.character().spells().buff_ids().collect();
        ids.into_iter()
            .find(|&id| self.with_ctx(|ctx| ctx.buff_ref(id).spell()) == spell)
            .unwrap_or_else(|| panic!("{}: no buff of spell {spell}", self.label))
    }

    /// Reads buff `name`.
    pub fn with_buff<R>(&mut self, name: &str, f: impl FnOnce(&Buff) -> R) -> R {
        let id = self.buff(name);
        self.with_buff_id(id, f)
    }

    pub fn with_buff_id<R>(&mut self, id: BuffId, f: impl FnOnce(&Buff) -> R) -> R {
        self.with_ctx(|ctx| f(ctx.buff_ref(id)))
    }

    /// The proc `name`.
    pub fn proc(&self, name: &str) -> ProcId {
        let procs = self.character().spells().procs().procs();
        procs
            .iter()
            .position(|proc| proc.name() == name)
            .map(|index| ProcId(index as u32))
            .unwrap_or_else(|| panic!("{}: no proc {name:?}", self.label))
    }

    /// The proc chance of proc `name` out of [`PROC_ROLL_RANGE`](crate::proc::PROC_ROLL_RANGE) for an event from `source`
    /// (`get_proc_range`).
    pub fn proc_range(&mut self, name: &str, source: ProcSource) -> u32 {
        let id = self.proc(name);
        let proc = self.character().spells().procs().get(id).clone();
        self.with_ctx(|ctx| proc.proc_range(source, ctx))
    }

    /// Whether proc `name` may fire on an event from `source` now: it listens to the source
    /// and its conditions hold (`proc_specific_conditions_fulfilled`).
    pub fn proc_conditions_fulfilled(&mut self, name: &str, source: ProcSource) -> bool {
        let id = self.proc(name);
        let proc = self.character().spells().procs().get(id).clone();
        proc.procs_from_source(source)
            && self.with_ctx(|ctx| proc.conditions_fulfilled(source, ctx))
    }

    /// Performs spell `id` (no availability checks, like the C++ `Spell::perform`) and runs
    /// what follows from it (procs, extra attacks).
    pub fn perform(&mut self, id: SpellId) -> CastReport {
        self.with_ctx(|ctx| ctx.cast(id))
    }

    /// [`Self::perform`] of the highest learned rank of spell `name`.
    pub fn cast(&mut self, name: &str) -> CastReport {
        let id = self.spell(name);
        self.perform(id)
    }

    /// The status of the highest learned rank of spell `name` (`get_spell_status`).
    pub fn status(&mut self, name: &str) -> SpellStatus {
        let id = self.spell(name);
        self.with_ctx(|ctx| ctx.with_spell(id, |spell, ctx| spell.status(ctx)))
    }

    /// Asserts the status of spell `name`.
    pub fn then_status_is(&mut self, name: &str, status: SpellStatus) {
        assert_eq!(
            self.status(name),
            status,
            "{}: status of {name}",
            self.label
        );
    }

    pub fn enable_spell(&mut self, name: &str) {
        let id = self.spell(name);
        self.with_ctx(|ctx| ctx.enable_spell(id));
        assert!(self.is_enabled(name));
    }

    pub fn disable_spell(&mut self, name: &str) {
        let id = self.spell(name);
        self.with_ctx(|ctx| ctx.disable_spell(id));
        assert!(!self.is_enabled(name));
    }

    pub fn is_enabled(&mut self, name: &str) -> bool {
        let id = self.spell(name);
        self.character().spells().spell(id).is_enabled()
    }

    /// The cooldown of spell `name` in seconds, three decimals (`get_base_cooldown`): the
    /// longer of its own and its category cooldown.
    pub fn base_cooldown(&mut self, name: &str) -> String {
        let id = self.spell(name);
        let seconds = self.with_ctx(|ctx| {
            ctx.with_spell(id, |spell, ctx| {
                spell
                    .cooldown_seconds(ctx)
                    .max(spell.category_cooldown_seconds(ctx))
            })
        });
        format!("{seconds:.3}")
    }

    pub fn cooldown_remaining(&mut self, name: &str) -> f64 {
        let id = self.spell(name);
        self.with_ctx(|ctx| ctx.with_spell(id, |spell, ctx| spell.cooldown_remaining(ctx)))
    }

    pub fn on_global_cooldown(&self) -> bool {
        self.character().on_global_cooldown(self.now())
    }

    pub fn on_stance_cooldown(&self) -> bool {
        self.character().on_stance_cooldown(self.now())
    }

    pub fn action_ready(&self) -> bool {
        self.character().action_ready(self.now())
    }

    /// Whether buff `name` is active.
    pub fn buff_is_active(&mut self, name: &str) -> bool {
        self.with_buff(name, Buff::is_active)
    }

    /// Lands on-next-swing spell `name` as if its main-hand swing came (`calculate_damage`),
    /// queued or not, and runs what follows from it.
    pub fn when_next_swing_spell_lands(&mut self, name: &str) -> CastReport {
        let id = self.spell(name);
        self.with_ctx(|ctx| {
            let report = ctx.perform_next_swing(id);
            ctx.perform_extra_attacks();
            report
        })
    }

    /// Applies buff `name` (`apply_buff`).
    pub fn apply_buff(&mut self, name: &str) {
        let id = self.buff(name);
        self.apply_buff_id(id);
    }

    pub fn apply_buff_id(&mut self, id: BuffId) {
        self.with_ctx(|ctx| SpellHost::apply_buff(ctx, id));
    }

    /// Removes buff `id` before it runs out (all its charges or stacks).
    pub fn cancel_buff_id(&mut self, id: BuffId) {
        self.with_ctx(|ctx| SpellHost::cancel_buff(ctx, id));
    }

    pub fn buff_charges(&mut self, name: &str) -> u32 {
        self.with_buff(name, Buff::charges)
    }

    pub fn buff_stacks(&mut self, name: &str) -> u32 {
        self.with_buff(name, Buff::stacks)
    }

    /// Swings `hand` now (`perform` of the auto attack): the swing timer restarts, the next
    /// swing is not scheduled.
    pub fn when_swing_is_performed(&mut self, hand: Hand) -> SwingReport {
        self.with_ctx(|ctx| ctx.perform_swing(hand))
    }

    /// When the next swing of `hand` is due, three decimals (`get_next_expected_use`).
    pub fn next_expected_use(&self, hand: Hand) -> String {
        let attack = self.character().spells().auto_attack(hand);
        format!("{:.3}", attack.next_expected_use(self.now()))
    }

    pub fn when_increasing_attack_speed(&mut self, percent: u32) {
        self.with_ctx(|ctx| EffectHost::increase_melee_attack_speed(ctx, percent));
    }

    pub fn when_decreasing_attack_speed(&mut self, percent: u32) {
        self.with_ctx(|ctx| EffectHost::decrease_melee_attack_speed(ctx, percent));
    }

    /// Starts auto attacking: the swings of each hand are scheduled from now.
    pub fn when_starting_attack(&mut self) {
        self.with_ctx(|ctx| SpellHost::start_attack(ctx));
    }

    // ---------------------------------------------------------------- equipment

    /// Equips item `item` in `slot` and checks that it took. The passives with equipment
    /// requirements (Two-Handed Weapon Specialization, Defiance) are re-evaluated, as the C++
    /// checked the weapon when it applied them.
    pub fn equip(&mut self, slot: EquipmentSlot, item: u32) {
        let db = &data().spells;
        self.with_ctx(|ctx| {
            let change = ctx.equip(db, slot, item);
            ctx.reevaluate_passives();
            change
        })
        .unwrap_or_else(|error| panic!("{}: equipping {item}: {error}", self.label));
        assert_eq!(self.character().equipment().item_id(slot), Some(item));
    }

    pub fn unequip(&mut self, slot: EquipmentSlot) {
        let db = &data().spells;
        self.with_ctx(|ctx| {
            ctx.unequip(db, slot);
            ctx.reevaluate_passives();
        });
        assert_eq!(self.character().equipment().item_id(slot), None);
    }

    /// A shield (The Immovable Object) in the off hand.
    pub fn given_a_shield_equipped(&mut self) {
        self.equip(EquipmentSlot::Offhand, 19321);
    }

    fn equip_weapon(&mut self, slot: EquipmentSlot, item: u32, min_max: Option<u32>, speed: f64) {
        self.equip(slot, item);
        let weapon = match slot {
            EquipmentSlot::Mainhand => self.character().equipment().mainhand(),
            EquipmentSlot::Offhand => self.character().equipment().offhand(),
            _ => self.character().equipment().ranged(),
        }
        .expect("a weapon");
        if let Some(damage) = min_max {
            assert_eq!((weapon.min_dmg(), weapon.max_dmg()), (damage, damage));
        }
        assert!((weapon.speed() - speed).abs() < 1e-9);
    }

    /// Equips the real item `item` in `slot` and checks its name and weapon type.
    fn equip_named(&mut self, slot: EquipmentSlot, item: u32, name: &str, weapon_type: WeaponType) {
        self.equip(slot, item);
        let equipped = self.character().equipment().item(slot).expect("equipped");
        assert_eq!(equipped.name(), name);
        assert_eq!(equipped.weapon_type(), Some(weapon_type));
    }

    pub fn given_a_mainhand_weapon_with_100_min_max_dmg(&mut self) {
        self.equip_weapon(EquipmentSlot::Mainhand, items::TEST_100_DMG, Some(100), 2.6);
    }

    pub fn given_a_mainhand_dagger_with_100_min_max_dmg(&mut self) {
        self.equip_weapon(
            EquipmentSlot::Mainhand,
            items::TEST_100_DMG_DAGGER,
            Some(100),
            2.0,
        );
    }

    pub fn given_a_mainhand_weapon_with_3_speed(&mut self) {
        self.equip_weapon(EquipmentSlot::Mainhand, items::TEST_3_SPEED, None, 3.0);
    }

    pub fn given_a_mainhand_weapon_with_2_speed(&mut self) {
        self.equip_weapon(EquipmentSlot::Mainhand, items::TEST_2_SPEED, None, 2.0);
    }

    pub fn given_a_twohand_weapon_with_100_min_max_dmg(&mut self) {
        self.equip_weapon(
            EquipmentSlot::Mainhand,
            items::TEST_100_DMG_2H,
            Some(100),
            3.5,
        );
    }

    pub fn given_an_offhand_weapon_with_100_min_max_dmg(&mut self) {
        self.equip_weapon(EquipmentSlot::Offhand, items::TEST_100_DMG, Some(100), 2.6);
    }

    pub fn given_an_offhand_weapon_with_3_speed(&mut self) {
        self.equip_weapon(EquipmentSlot::Offhand, items::TEST_3_SPEED, None, 3.0);
    }

    pub fn given_an_offhand_weapon_with_2_speed(&mut self) {
        self.equip_weapon(EquipmentSlot::Offhand, items::TEST_2_SPEED, None, 2.0);
    }

    pub fn given_an_offhand_axe(&mut self) {
        self.equip_weapon(EquipmentSlot::Offhand, items::TEST_AXE, None, 2.0);
        let weapon = self.character().equipment().offhand().expect("a weapon");
        assert_eq!(weapon.weapon_type(), WeaponType::Axe);
    }

    pub fn given_a_ranged_weapon_with_100_min_max_dmg(&mut self) {
        self.equip_weapon(
            EquipmentSlot::Ranged,
            items::TEST_100_DMG_RANGED,
            Some(100),
            2.6,
        );
    }

    pub fn given_a_ranged_weapon_with_3_speed(&mut self) {
        self.equip_weapon(EquipmentSlot::Ranged, items::TEST_3_SPEED_RANGED, None, 3.0);
    }

    pub fn given_a_ranged_weapon_with_2_speed(&mut self) {
        self.equip_weapon(EquipmentSlot::Ranged, items::TEST_2_SPEED_RANGED, None, 2.0);
    }

    pub fn given_no_mainhand(&mut self) {
        self.unequip(EquipmentSlot::Mainhand);
    }

    pub fn given_no_offhand(&mut self) {
        self.unequip(EquipmentSlot::Offhand);
    }

    pub fn given_1h_axe_equipped_in_mainhand(&mut self) {
        self.equip_named(
            EquipmentSlot::Mainhand,
            18828,
            "High Warlord's Cleaver",
            WeaponType::Axe,
        );
    }

    pub fn given_1h_mace_equipped_in_mainhand(&mut self) {
        self.equip_named(
            EquipmentSlot::Mainhand,
            18866,
            "High Warlord's Bludgeon",
            WeaponType::Mace,
        );
    }

    pub fn given_1h_sword_equipped_in_mainhand(&mut self) {
        self.equip_named(
            EquipmentSlot::Mainhand,
            12584,
            "Grand Marshal's Longsword",
            WeaponType::Sword,
        );
    }

    pub fn given_fist_weapon_equipped_in_mainhand(&mut self) {
        self.equip_named(
            EquipmentSlot::Mainhand,
            18844,
            "High Warlord's Right Claw",
            WeaponType::Fist,
        );
    }

    pub fn given_dagger_equipped_in_mainhand(&mut self) {
        self.equip_named(
            EquipmentSlot::Mainhand,
            12783,
            "Heartseeker",
            WeaponType::Dagger,
        );
    }

    pub fn given_1h_axe_equipped_in_offhand(&mut self) {
        self.equip_named(
            EquipmentSlot::Offhand,
            18828,
            "High Warlord's Cleaver",
            WeaponType::Axe,
        );
    }

    pub fn given_1h_mace_equipped_in_offhand(&mut self) {
        self.equip_named(
            EquipmentSlot::Offhand,
            18866,
            "High Warlord's Bludgeon",
            WeaponType::Mace,
        );
    }

    pub fn given_1h_sword_equipped_in_offhand(&mut self) {
        self.equip_named(
            EquipmentSlot::Offhand,
            12584,
            "Grand Marshal's Longsword",
            WeaponType::Sword,
        );
    }

    pub fn given_fist_weapon_equipped_in_offhand(&mut self) {
        self.equip_named(
            EquipmentSlot::Offhand,
            18848,
            "High Warlord's Left Claw",
            WeaponType::Fist,
        );
    }

    pub fn given_dagger_equipped_in_offhand(&mut self) {
        self.equip_named(
            EquipmentSlot::Offhand,
            12783,
            "Heartseeker",
            WeaponType::Dagger,
        );
    }

    pub fn given_2h_axe_equipped(&mut self) {
        self.equip_named(
            EquipmentSlot::Mainhand,
            12784,
            "Arcanite Reaper",
            WeaponType::TwohandAxe,
        );
    }

    pub fn given_2h_mace_equipped(&mut self) {
        self.equip_named(
            EquipmentSlot::Mainhand,
            17073,
            "Earthshaker",
            WeaponType::TwohandMace,
        );
    }

    pub fn given_2h_sword_equipped(&mut self) {
        self.equip_named(
            EquipmentSlot::Mainhand,
            18876,
            "Grand Marshal's Claymore",
            WeaponType::TwohandSword,
        );
    }

    pub fn given_polearm_equipped(&mut self) {
        self.equip_named(
            EquipmentSlot::Mainhand,
            18869,
            "Grand Marshal's Glaive",
            WeaponType::Polearm,
        );
    }

    pub fn given_staff_equipped(&mut self) {
        self.equip_named(
            EquipmentSlot::Mainhand,
            18873,
            "Grand Marshal's Stave",
            WeaponType::Staff,
        );
    }

    // ---------------------------------------------------------------- weapon skill

    pub fn mh_weapon_skill(&self) -> u32 {
        self.stat(|stats, ctx| stats.get_mh_wpn_skill(ctx))
    }

    pub fn oh_weapon_skill(&self) -> u32 {
        self.stat(|stats, ctx| stats.get_oh_wpn_skill(ctx))
    }

    fn given_sword_skill_ring(&mut self, ring: u32) {
        self.equip(EquipmentSlot::Ring1, ring);
    }

    pub fn given_300_weapon_skill_mh(&mut self) {
        assert_eq!(self.mh_weapon_skill(), 300);
    }

    pub fn given_305_weapon_skill_mh(&mut self) {
        self.given_sword_skill_ring(items::TEST_5_SWORD_SKILL);
        assert_eq!(self.mh_weapon_skill(), 305);
    }

    pub fn given_310_weapon_skill_mh(&mut self) {
        self.given_sword_skill_ring(items::TEST_10_SWORD_SKILL);
        assert_eq!(self.mh_weapon_skill(), 310);
    }

    pub fn given_315_weapon_skill_mh(&mut self) {
        self.given_sword_skill_ring(items::TEST_15_SWORD_SKILL);
        assert_eq!(self.mh_weapon_skill(), 315);
    }

    pub fn given_300_weapon_skill_oh(&mut self) {
        assert_eq!(self.oh_weapon_skill(), 300);
    }

    pub fn given_305_weapon_skill_oh(&mut self) {
        self.given_sword_skill_ring(items::TEST_5_SWORD_SKILL);
        assert_eq!(self.oh_weapon_skill(), 305);
    }

    pub fn given_310_weapon_skill_oh(&mut self) {
        self.given_sword_skill_ring(items::TEST_10_SWORD_SKILL);
        assert_eq!(self.oh_weapon_skill(), 310);
    }

    pub fn given_315_weapon_skill_oh(&mut self) {
        self.given_sword_skill_ring(items::TEST_15_SWORD_SKILL);
        assert_eq!(self.oh_weapon_skill(), 315);
    }

    // ---------------------------------------------------------------- guaranteed outcomes

    /// Makes crits impossible (hit, block) or certain (crit), as the C++ tests did through the
    /// crit penalty and the melee aura crit.
    /// The previous forced outcome's adjustment is undone first, so outcomes can be switched.
    fn force_crit(&mut self, outcome: Outcome) {
        match self.forced_crit.take() {
            Some(Outcome::Crit) => self.stats_mut().decrease_melee_aura_crit(999_999),
            Some(_) => self.stats_mut().decrease_crit_penalty(999_999),
            None => {}
        }
        match outcome {
            Outcome::Hit | Outcome::Block => self.stats_mut().increase_crit_penalty(999_999),
            Outcome::Crit => self.stats_mut().increase_melee_aura_crit(999_999),
            _ => return,
        }
        self.forced_crit = Some(outcome);
    }

    /// Reshapes the white (`white`) or special hit tables of both hands' weapon skills so that
    /// every roll lands on `outcome`, then checks every roll. The roll context is refreshed
    /// first so the next roll does not recompute the miss ranges; equip before forcing.
    fn force_melee_tables(&mut self, outcome: Outcome, white: bool) {
        assert!(
            white || outcome != Outcome::Glancing,
            "special attacks cannot glance"
        );
        self.force_crit(outcome);
        self.reshape_melee_tables(outcome, white);
        let skills = [self.mh_weapon_skill(), self.oh_weapon_skill()];
        for skill in skills {
            self.assert_melee_table_can_only(skill, white, outcome);
        }
    }

    /// Reshapes the special hit tables so that main-hand abilities always land on `mh` and
    /// off-hand abilities on `oh`. The hands must use different weapon skills (the tables are
    /// per skill); crits are impossible.
    fn force_special_tables_per_hand(&mut self, mh: Outcome, oh: Outcome) {
        assert!(
            ![mh, oh].contains(&Outcome::Crit) && ![mh, oh].contains(&Outcome::Glancing),
            "no forced crits or glancing blows per hand"
        );
        let (mh_skill, oh_skill) = (self.mh_weapon_skill(), self.oh_weapon_skill());
        assert_ne!(mh_skill, oh_skill, "the hands share one special table");
        self.force_crit(Outcome::Hit);
        self.reshape_melee_tables_for(&[mh_skill], mh, false);
        self.reshape_melee_tables_for(&[oh_skill], oh, false);
        self.assert_melee_table_can_only(mh_skill, false, mh);
        self.assert_melee_table_can_only(oh_skill, false, oh);
    }

    /// Main-hand abilities are always dodged, off-hand abilities always hit (a 315 sword
    /// skill main hand and an axe off hand are equipped).
    pub fn given_a_mainhand_ability_dodge_and_an_offhand_ability_hit(&mut self) {
        self.given_mainhand_and_offhand_of_different_skills();
        self.force_special_tables_per_hand(Outcome::Dodge, Outcome::Hit);
    }

    /// Main-hand abilities always hit, off-hand abilities are always dodged (a 315 sword
    /// skill main hand and an axe off hand are equipped).
    pub fn given_a_mainhand_ability_hit_and_an_offhand_ability_dodge(&mut self) {
        self.given_mainhand_and_offhand_of_different_skills();
        self.force_special_tables_per_hand(Outcome::Hit, Outcome::Dodge);
    }

    fn given_mainhand_and_offhand_of_different_skills(&mut self) {
        self.given_a_mainhand_weapon_with_100_min_max_dmg();
        self.given_an_offhand_axe();
        self.given_315_weapon_skill_mh();
        self.given_300_weapon_skill_oh();
    }

    /// Sets the miss, dodge, parry, glancing and block ranges of the white or special tables
    /// of both hands' weapon skills to all or nothing of the roll range.
    fn reshape_melee_tables(&mut self, outcome: Outcome, white: bool) {
        let skills = [self.mh_weapon_skill(), self.oh_weapon_skill()];
        self.reshape_melee_tables_for(&skills, outcome, white);
    }

    /// [`Self::reshape_melee_tables`] for the tables of weapon skills `skills`.
    fn reshape_melee_tables_for(&mut self, skills: &[u32], outcome: Outcome, white: bool) {
        let view = self.view();
        let character = self.character_mut();
        let ctx = character.refresh_roll_context(&view);
        let miss = if outcome == Outcome::Miss {
            ROLL_RANGE
        } else {
            0
        };
        for &skill in skills {
            if white {
                let table = character.roll_mut().melee_white_table_mut(&ctx, skill);
                table.update_miss_chance(miss);
                table.update_dodge_chance(outcome.chance(Outcome::Dodge));
                table.update_parry_chance(outcome.chance(Outcome::Parry));
                table.update_glancing_chance(outcome.chance(Outcome::Glancing));
                table.update_block_chance(outcome.chance(Outcome::Block));
            } else {
                let table = character.roll_mut().melee_special_table_mut(&ctx, skill);
                table.update_miss_chance(miss);
                table.update_dodge_chance(outcome.chance(Outcome::Dodge));
                table.update_parry_chance(outcome.chance(Outcome::Parry));
                table.update_block_chance(outcome.chance(Outcome::Block));
            }
        }
    }

    /// Checks that every roll of the white or special table of `skill` gives `outcome`.
    /// Port of the `assert_melee_*_table_can_only_*` family.
    fn assert_melee_table_can_only(&mut self, skill: u32, white: bool, outcome: Outcome) {
        let label = self.label.clone();
        let view = self.view();
        let crit = self.stat(|stats, ctx| stats.get_mh_crit_chance(ctx));
        let character = self.character_mut();
        let clvl = character.clvl();
        let ctx = character.refresh_roll_context(&view);
        let roll = character.roll_mut();
        let crit = roll.get_suppressed_crit(clvl, crit);
        let mut random = Random::from_seed(0, ROLL_RANGE, SEED);
        for value in 0..ROLL_RANGE {
            let result = if white {
                roll.melee_white_table_mut(&ctx, skill).get_outcome(
                    &mut random,
                    value,
                    crit,
                    IncludedOutcomes::ALL,
                )
            } else {
                roll.melee_special_table_mut(&ctx, skill).get_outcome(
                    &mut random,
                    value,
                    crit,
                    IncludedOutcomes::ALL,
                )
            };
            assert_eq!(
                result,
                outcome.result(),
                "{label}: roll {value} of the {} table for skill {skill}",
                if white { "white" } else { "special" }
            );
        }
    }

    pub fn given_a_guaranteed_white_hit(&mut self) {
        self.force_melee_tables(Outcome::Hit, true);
    }

    pub fn given_a_guaranteed_white_glancing_blow(&mut self) {
        self.force_melee_tables(Outcome::Glancing, true);
    }

    pub fn given_a_guaranteed_white_crit(&mut self) {
        self.force_melee_tables(Outcome::Crit, true);
    }

    pub fn given_a_guaranteed_white_miss(&mut self) {
        self.force_melee_tables(Outcome::Miss, true);
    }

    pub fn given_a_guaranteed_white_dodge(&mut self) {
        self.force_melee_tables(Outcome::Dodge, true);
    }

    pub fn given_a_guaranteed_white_parry(&mut self) {
        self.force_melee_tables(Outcome::Parry, true);
    }

    pub fn given_a_guaranteed_white_block(&mut self) {
        self.force_melee_tables(Outcome::Block, true);
    }

    pub fn given_a_guaranteed_melee_ability_hit(&mut self) {
        self.force_melee_tables(Outcome::Hit, false);
    }

    pub fn given_a_guaranteed_melee_ability_crit(&mut self) {
        self.force_melee_tables(Outcome::Crit, false);
    }

    pub fn given_a_guaranteed_melee_ability_miss(&mut self) {
        self.force_melee_tables(Outcome::Miss, false);
    }

    pub fn given_a_guaranteed_melee_ability_dodge(&mut self) {
        self.force_melee_tables(Outcome::Dodge, false);
    }

    pub fn given_a_guaranteed_melee_ability_parry(&mut self) {
        self.force_melee_tables(Outcome::Parry, false);
    }

    pub fn given_a_guaranteed_melee_ability_block(&mut self) {
        self.force_melee_tables(Outcome::Block, false);
    }

    /// Special attacks can neither miss nor be avoided, and crit as often as the crit chance
    /// says (a guaranteed hit without the crit penalty).
    pub fn given_no_melee_ability_avoidance(&mut self) {
        self.reshape_melee_tables(Outcome::Hit, false);
    }

    // ---------------------------------------------------------------- stats and target

    pub fn given_1000_melee_ap(&mut self) {
        let view = self.view();
        let ap = self.character().melee_ap(&view);
        if ap < 1000 {
            self.stats_mut().increase_melee_ap(1000 - ap);
        } else {
            self.stats_mut().decrease_melee_ap(ap - 1000);
        }
        assert_eq!(self.character().melee_ap(&view), 1000);
    }

    /// Moves a primary stat to `value` through its increase / decrease pair.
    fn given_stat(
        &mut self,
        value: u32,
        get: fn(&CharacterStats, &StatContext) -> u32,
        increase: fn(&mut CharacterStats, u32),
        decrease: fn(&mut CharacterStats, u32),
    ) {
        let current = self.stat(get);
        if current < value {
            increase(self.stats_mut(), value - current);
        } else {
            decrease(self.stats_mut(), current - value);
        }
        assert_eq!(self.stat(get), value);
    }

    pub fn given_character_has_strength(&mut self, value: u32) {
        self.given_stat(
            value,
            CharacterStats::get_strength,
            CharacterStats::increase_strength,
            CharacterStats::decrease_strength,
        );
    }

    pub fn given_character_has_agility(&mut self, value: u32) {
        self.given_stat(
            value,
            CharacterStats::get_agility,
            CharacterStats::increase_agility,
            CharacterStats::decrease_agility,
        );
    }

    pub fn given_character_has_stamina(&mut self, value: u32) {
        self.given_stat(
            value,
            CharacterStats::get_stamina,
            CharacterStats::increase_stamina,
            CharacterStats::decrease_stamina,
        );
    }

    pub fn given_character_has_intellect(&mut self, value: u32) {
        self.given_stat(
            value,
            CharacterStats::get_intellect,
            CharacterStats::increase_intellect,
            CharacterStats::decrease_intellect,
        );
    }

    pub fn given_character_has_spirit(&mut self, value: u32) {
        self.given_stat(
            value,
            CharacterStats::get_spirit,
            CharacterStats::increase_spirit,
            CharacterStats::decrease_spirit,
        );
    }

    pub fn given_target_has_0_armor(&mut self) {
        self.target_mut().set_base_armor(0);
        assert_eq!(self.target().armor(), 0);
    }

    pub fn given_target_is_beast(&mut self) {
        self.target_mut().set_creature_type(CreatureType::Beast);
    }

    pub fn given_target_is_humanoid(&mut self) {
        self.target_mut().set_creature_type(CreatureType::Humanoid);
    }

    pub fn given_in_melee_attack_mode(&mut self) {
        self.character_mut()
            .spells_mut()
            .set_attack_mode(AttackMode::MeleeAttack);
    }

    pub fn given_in_ranged_attack_mode(&mut self) {
        self.character_mut()
            .spells_mut()
            .set_attack_mode(AttackMode::RangedAttack);
    }

    // ---------------------------------------------------------------- talents

    /// Puts `rank` points into talent `talent` of tab `tab` (e.g. `"Fury"`), ignoring the tier
    /// unlock and the point budget like the C++ tests did; a prerequisite must be maxed first.
    pub fn given_talent_rank(&mut self, tab: &str, talent: &str, rank: u32) {
        self.given_talent_rank_of(self.id, tab, talent, rank);
    }

    /// [`Self::given_talent_rank`] for character `id` of the raid.
    pub fn given_talent_rank_of(&mut self, id: CharId, tab: &str, talent: &str, rank: u32) {
        assert!(rank > 0);
        let talents = self
            .raid
            .character(id)
            .talents()
            .unwrap_or_else(|| panic!("{}: the class has no talents", self.label));
        let skill_line = talents
            .file()
            .tabs
            .iter()
            .find(|t| t.name == tab)
            .unwrap_or_else(|| panic!("{}: no talent tab {tab:?}", self.label))
            .skill_line;
        let node = talents
            .node_of_name(talent, Some(skill_line))
            .unwrap_or_else(|| panic!("{}: no talent {talent:?} in {tab}", self.label));
        for i in 0..rank {
            let change = self
                .raid
                .character_mut(id)
                .talents_mut()
                .and_then(|t| t.force_increment_rank(node))
                .unwrap_or_else(|| {
                    panic!(
                        "Failed to increment {talent} to rank {}, does it have parent talents?",
                        i + 1
                    )
                });
            self.raid
                .with_character(id, |ctx| ctx.apply_talent_changes([change]));
        }
    }

    pub fn given_talent_ranks(&mut self, tab: &str, ranks: &[(&str, u32)]) {
        for &(talent, rank) in ranks {
            self.given_talent_rank(tab, talent, rank);
        }
    }

    // ---------------------------------------------------------------- engine

    /// Sets the clock to `time`: forward keeps the queue, backward starts a new iteration.
    pub fn given_engine_priority_at(&mut self, time: f64) {
        let engine = self.raid.engine_mut();
        if time < engine.current_time() {
            engine.prepare_iteration(time);
        } else {
            engine.advance_time_to(time);
        }
    }

    /// Moves the clock `delay` seconds forward and drops every queued event.
    pub fn given_engine_priority_pushed_forward(&mut self, delay: f64) {
        let engine = self.raid.engine_mut();
        let time = engine.current_time() + delay;
        engine.prepare_iteration(time);
    }

    /// Events of `event_type` are dropped instead of run from now on.
    pub fn given_event_is_ignored(&mut self, event_type: EventType) {
        self.ignored_events.insert(event_type);
    }

    /// Runs the queued events that come before `time` (an event at `time` is left queued).
    ///
    /// # Panics
    /// Panics if the queue runs dry before `time`.
    pub fn when_running_queued_events_until(&mut self, time: f64) {
        while self.now() < time {
            let Some(next) = self.raid.engine().peek() else {
                panic!(
                    "{}: attempted to run queued events until {time:.3} but ran out of events \
                     at {:.3}",
                    self.label,
                    self.now()
                );
            };
            if next.time > time || (next.time - time).abs() < 1e-6 {
                break;
            }
            let event = self.raid.engine_mut().next_event().expect("peeked");
            if !self.ignored_events.contains(&event.kind.event_type()) {
                self.raid.dispatch(&event);
            }
        }
    }

    /// Drops queued events until one of `event_type`, which is dispatched; returns its time.
    ///
    /// # Panics
    /// Panics if the queue runs dry first.
    pub fn when_running_until_event(&mut self, event_type: EventType) -> f64 {
        loop {
            let event = self.raid.engine_mut().next_event().unwrap_or_else(|| {
                panic!(
                    "{}: ran out of events waiting for {event_type:?}",
                    self.label
                )
            });
            if event.kind.event_type() == event_type {
                self.raid.dispatch(&event);
                return event.time;
            }
        }
    }

    /// Runs only the queued events of `event_type` until the queue is empty; the others are
    /// dropped.
    pub fn when_running_only(&mut self, event_type: EventType) {
        while let Some(event) = self.raid.engine_mut().next_event() {
            if event.kind.event_type() == event_type {
                self.raid.dispatch(&event);
            }
        }
    }

    /// Pops the next event that is not ignored.
    fn next_event(&mut self) -> Option<Event> {
        loop {
            let event = self.raid.engine_mut().next_event()?;
            if !self.ignored_events.contains(&event.kind.event_type()) {
                return Some(event);
            }
        }
    }

    /// Pops the next event that is not ignored and checks its type and time (`"1.500"`, three
    /// decimals). With `act` ([`RUN_EVENT`]) the event is also dispatched.
    pub fn then_next_event_is(&mut self, event_type: EventType, time: &str, act: bool) {
        let event = self.next_event().unwrap_or_else(|| {
            panic!(
                "{}: queue empty, expected {event_type:?} at {time}",
                self.label
            )
        });
        assert_eq!(
            event.kind.event_type(),
            event_type,
            "{}: expected {event_type:?} at {time} but got {:?} at {:.3}",
            self.label,
            event.kind,
            event.time
        );
        assert_eq!(
            format!("{:.3}", event.time),
            time,
            "{}: {event_type:?} at the wrong time",
            self.label
        );
        if act {
            self.raid.dispatch(&event);
        }
    }

    /// The queued events in order, for debugging (`dump_queued_events`). Leaves the queue as
    /// it is.
    pub fn queued_events(&self) -> Vec<Event> {
        let mut queue = self.raid.engine().queue().clone();
        std::iter::from_fn(|| queue.pop()).collect()
    }

    // ---------------------------------------------------------------- statistics

    pub fn damage_dealt(&self) -> u64 {
        self.character().statistics().total_damage()
    }

    /// The damage of spell `name`, every rank (`get_total_damage_for_spell`).
    pub fn damage_dealt_by(&self, name: &str) -> u64 {
        self.character()
            .statistics()
            .spells()
            .filter(|(key, _)| key.name == name)
            .map(|(_, statistics)| statistics.total_damage())
            .sum()
    }

    pub fn threat_dealt(&self) -> u64 {
        self.character().statistics().total_threat()
    }

    pub fn given_no_previous_damage_dealt(&self) {
        self.then_damage_dealt_is(0);
    }

    pub fn then_damage_dealt_is(&self, damage: u64) {
        assert_eq!(
            self.damage_dealt(),
            damage,
            "{}: then_damage_dealt_is",
            self.label
        );
    }

    pub fn then_damage_dealt_is_in_range(&self, min: u64, max: u64) {
        assert!(min < max);
        let damage = self.damage_dealt();
        assert!(
            (min..=max).contains(&damage),
            "{}: expected damage in {min} - {max} but got {damage}",
            self.label
        );
    }

    pub fn then_threat_dealt_is(&self, threat: u64) {
        assert_eq!(
            self.threat_dealt(),
            threat,
            "{}: then_threat_dealt_is",
            self.label
        );
    }

    pub fn then_threat_dealt_is_in_range(&self, min: u64, max: u64) {
        assert!(min < max);
        let threat = self.threat_dealt();
        assert!(
            (min..=max).contains(&threat),
            "{}: expected threat in {min} - {max} but got {threat}",
            self.label
        );
    }

    pub fn then_resource_is(&self, resource: ResourceType, expected: u32) {
        assert_eq!(
            self.character().resource_level(resource),
            expected,
            "{}: {resource:?}",
            self.label
        );
    }
}
