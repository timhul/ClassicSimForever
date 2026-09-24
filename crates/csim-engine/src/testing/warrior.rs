//! The Warrior layer of the harness: an Orc Warrior with 100 rage, stance and GCD helpers and
//! the Warrior talent shortcuts. Port of `Test/Warrior/TestSpellWarrior`.

use std::ops::{Deref, DerefMut};

use super::SpellTest;
use crate::character::STANCE_COOLDOWN;
use crate::faction::PlayerClass;
use crate::race::Race;
use crate::resource::ResourceType;
use crate::spell::dbc::SpellModOp;
use crate::stance::Stance;

/// A level 60 Orc Warrior with 100 rage. Derefs to the class-agnostic [`SpellTest`].
pub(crate) struct WarriorTest {
    base: SpellTest,
}

impl Deref for WarriorTest {
    type Target = SpellTest;

    fn deref(&self) -> &SpellTest {
        &self.base
    }
}

impl DerefMut for WarriorTest {
    fn deref_mut(&mut self) -> &mut SpellTest {
        &mut self.base
    }
}

impl WarriorTest {
    /// A Warrior prepared for a set of iterations (`set_up()`).
    pub fn new(label: &str) -> Self {
        let mut test = Self::unprepared(label);
        test.prepare_set_of_combat_iterations();
        test
    }

    /// A Warrior whose set of iterations is not prepared yet (`set_up(false)`), for tests
    /// that change the setup first.
    pub fn unprepared(label: &str) -> Self {
        let mut base = SpellTest::new(PlayerClass::Warrior, Race::Orc, label);
        base.character_mut().gain_resource(ResourceType::Rage, 100);
        WarriorTest { base }
    }

    pub fn rage(&self) -> u32 {
        self.character().resource_level(ResourceType::Rage)
    }

    // ---------------------------------------------------------------- talents

    pub fn given_arms_talent_with_rank(&mut self, talent: &str, rank: u32) {
        self.given_talent_rank("Arms", talent, rank);
    }

    pub fn given_fury_talent_with_rank(&mut self, talent: &str, rank: u32) {
        self.given_talent_rank("Fury", talent, rank);
    }

    pub fn given_protection_talent_with_rank(&mut self, talent: &str, rank: u32) {
        self.given_talent_rank("Protection", talent, rank);
    }

    /// The percent crit damage bonus the spell modifiers give Heroic Strike (every Warrior
    /// ability shares its class mask bit with Impale's).
    fn ability_crit_damage_pct(&mut self) -> f64 {
        let id = self.spell("Heroic Strike");
        let character = self.character();
        let record = character.spells().spell(id).record();
        character.spell_modifiers().pct(
            record.class_options.as_ref(),
            SpellModOp::CritDamageAndHealing,
        )
    }

    /// `rank` of 2 Impale: +10 % critical strike damage bonus of abilities per rank (a spell
    /// modifier in Forever; the C++ raised the crit damage stat).
    pub fn given_impale(&mut self, rank: u32) {
        assert_eq!(self.ability_crit_damage_pct(), 0.0);
        if rank > 0 {
            self.given_arms_talent_with_rank("Impale", rank);
        }
        assert_eq!(self.ability_crit_damage_pct(), 10.0 * f64::from(rank));
    }

    /// `rank` of 5 Improved Tactical Mastery (Forever's Tactical Mastery talent): 3 more rage
    /// kept through a stance change per rank, on top of what the character keeps untalented.
    pub fn given_tactical_mastery(&mut self, rank: u32) {
        let untalented = self.character().stance_rage_retained();
        if rank > 0 {
            self.given_arms_talent_with_rank("Improved Tactical Mastery", rank);
        }
        assert_eq!(
            self.character().stance_rage_retained(),
            untalented + 3 * rank
        );
    }

    // ---------------------------------------------------------------- stances

    /// Puts the Warrior in `stance` through its stance spell, with the stance cooldown and
    /// the GCD over.
    fn given_warrior_in_stance(&mut self, stance: Stance, spell: &str) {
        if self.character().stance() != stance {
            self.cast(spell);
            self.given_engine_priority_pushed_forward(STANCE_COOLDOWN + 1.0);
        }
        let now = self.now();
        assert!(!self.character().on_stance_cooldown(now));
        assert_eq!(self.character().stance(), stance);
    }

    pub fn given_warrior_in_battle_stance(&mut self) {
        self.given_warrior_in_stance(Stance::Battle, "Battle Stance");
    }

    pub fn given_warrior_in_berserker_stance(&mut self) {
        self.given_warrior_in_stance(Stance::Berserker, "Berserker Stance");
    }

    pub fn given_warrior_in_defensive_stance(&mut self) {
        self.given_warrior_in_stance(Stance::Defensive, "Defensive Stance");
    }

    pub fn when_switching_to_battle_stance(&mut self) {
        self.cast("Battle Stance");
    }

    pub fn when_switching_to_berserker_stance(&mut self) {
        self.cast("Berserker Stance");
    }

    pub fn when_switching_to_defensive_stance(&mut self) {
        self.cast("Defensive Stance");
    }

    // ---------------------------------------------------------------- GCD and rage

    /// Puts the Warrior on the GCD with a Whirlwind (equipping a main hand if there is none).
    /// Rage is left as it was.
    pub fn given_warrior_is_on_gcd(&mut self) {
        if !self.character().has_mainhand() {
            self.given_a_mainhand_weapon_with_100_min_max_dmg();
        }
        self.given_warrior_is_on_gcd_from("Whirlwind");
    }

    /// Puts the Warrior on the GCD by performing spell `name` with full rage; the rage it cost
    /// or gave is undone.
    pub fn given_warrior_is_on_gcd_from(&mut self, name: &str) {
        let before = self.rage();
        let now = self.now();
        self.character_mut()
            .gain_resource(ResourceType::Rage, 100 - before);
        self.cast(name);
        let after = self.rage();
        if after < before {
            self.character_mut()
                .gain_resource(ResourceType::Rage, before - after);
        } else {
            self.character_mut()
                .lose_resource(ResourceType::Rage, after - before, now);
        }
        assert_eq!(self.rage(), before);
        assert!(self.character().on_global_cooldown(now));
    }

    pub fn given_warrior_has_rage(&mut self, rage: u32) {
        let now = self.now();
        let current = self.rage();
        self.character_mut()
            .lose_resource(ResourceType::Rage, current, now);
        self.character_mut().gain_resource(ResourceType::Rage, rage);
        self.then_warrior_has_rage(rage);
    }

    pub fn then_warrior_has_rage(&self, rage: u32) {
        self.then_resource_is(ResourceType::Rage, rage);
    }

    // ---------------------------------------------------------------- overpower

    /// Makes Overpower usable: a special attack is dodged (the Warrior's combo point).
    pub fn given_overpower_is_active(&mut self) {
        self.given_a_mainhand_weapon_with_100_min_max_dmg();
        self.given_a_guaranteed_melee_ability_dodge();
        self.given_warrior_is_on_gcd();
        let gcd = self.character().global_cooldown();
        self.given_engine_priority_pushed_forward(gcd);
        self.then_overpower_is_active();
    }

    pub fn then_overpower_is_active(&self) {
        assert!(self.character().combo_points() > 0, "Overpower is inactive");
    }

    pub fn then_overpower_is_inactive(&self) {
        assert_eq!(self.character().combo_points(), 0, "Overpower is active");
    }
}
