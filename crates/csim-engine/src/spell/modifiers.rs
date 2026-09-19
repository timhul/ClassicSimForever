//! Spell modifiers: the `ADD_FLAT_MODIFIER` / `ADD_PCT_MODIFIER` auras of talents and set
//! bonuses (`data/TALENT_INSTRUCTIONS.md` §1.6 B).
//!
//! A modifier aura names an operation (`EffectMiscValue_0` = [`SpellModOp`]), an amount
//! (`EffectBasePointsF`) and the spells it applies to (`EffectSpellClassMask_0..3`, matched
//! against `SpellClassOptions.SpellClassMask_*` within the same `SpellClassSet`). A character
//! keeps the active ones in a [`SpellModifiers`] table; [`crate::spell::Spell`] asks the table
//! for its cost, crit chance, cast time, cooldown, damage and effect values. This replaces the
//! ClassicSim `modified_by_talent` lists: talents no longer know spells by name, and spells no
//! longer know talents at all.

use crate::spell::dbc::SpellModOp;
use crate::spell::record::{ClassOptions, EffectRecord};

/// One active modifier aura effect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpellModifier {
    /// The spell family of the aura's spell (`SpellClassOptions.SpellClassSet`).
    pub set: u32,
    /// `EffectSpellClassMask_0..3`: the spells the modifier applies to.
    pub class_mask: [u32; 4],
    pub op: SpellModOp,
    /// `ADD_PCT_MODIFIER` (percent) rather than `ADD_FLAT_MODIFIER` (raw units).
    pub pct: bool,
    /// `EffectBasePointsF` at the talent's current rank.
    pub amount: f64,
    /// The spell that owns the aura, so it can be removed again.
    pub source: u32,
}

impl SpellModifier {
    /// Builds the modifier described by `effect` of spell `source` in family `set`.
    pub fn from_effect(effect: &EffectRecord, source: u32, set: u32, amount: f64) -> Self {
        SpellModifier {
            set,
            class_mask: effect.spell_class_mask,
            op: effect.mod_op(),
            pct: effect.aura == crate::spell::dbc::AuraType::AddPctModifier,
            amount,
            source,
        }
    }

    /// Whether the modifier applies to a spell with `class_options`.
    pub fn applies_to(&self, class_options: Option<&ClassOptions>) -> bool {
        class_options.is_some_and(|c| c.matches(self.set, &self.class_mask))
    }
}

/// The active spell modifiers of one character.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpellModifiers {
    modifiers: Vec<SpellModifier>,
}

impl SpellModifiers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, modifier: SpellModifier) {
        self.modifiers.push(modifier);
    }

    /// Removes one modifier equal to `modifier`; returns whether one was found.
    pub fn remove(&mut self, modifier: &SpellModifier) -> bool {
        match self.modifiers.iter().position(|m| m == modifier) {
            Some(index) => {
                self.modifiers.remove(index);
                true
            }
            None => false,
        }
    }

    /// Removes every modifier owned by spell `source`.
    pub fn remove_source(&mut self, source: u32) {
        self.modifiers.retain(|m| m.source != source);
    }

    pub fn len(&self) -> usize {
        self.modifiers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modifiers.is_empty()
    }

    pub fn all(&self) -> &[SpellModifier] {
        &self.modifiers
    }

    /// The modifiers of `op` that apply to a spell with `class` options.
    pub fn applicable<'a>(
        &'a self,
        class: Option<&'a ClassOptions>,
        op: SpellModOp,
    ) -> impl Iterator<Item = &'a SpellModifier> + 'a {
        self.modifiers
            .iter()
            .filter(move |m| m.op == op && m.applies_to(class))
    }

    /// Sum of the flat amounts of `op` for a spell with `class` options.
    pub fn flat(&self, class: Option<&ClassOptions>, op: SpellModOp) -> f64 {
        self.applicable(class, op)
            .filter(|m| !m.pct)
            .map(|m| m.amount)
            .sum()
    }

    /// Sum of the percent amounts of `op` for a spell with `class` options.
    pub fn pct(&self, class: Option<&ClassOptions>, op: SpellModOp) -> f64 {
        self.applicable(class, op)
            .filter(|m| m.pct)
            .map(|m| m.amount)
            .sum()
    }

    /// `1 + pct / 100` for `op`.
    pub fn multiplier(&self, class: Option<&ClassOptions>, op: SpellModOp) -> f64 {
        1.0 + self.pct(class, op) / 100.0
    }

    /// Modifies `base` by the flat and percent modifiers of `op`: `(base + flat) × (1 + pct)`.
    pub fn apply(&self, class: Option<&ClassOptions>, op: SpellModOp, base: f64) -> f64 {
        (base + self.flat(class, op)) * self.multiplier(class, op)
    }

    /// The operation that modifies the value of effect `index` (`POINTS_INDEX_0..4`), if any.
    pub fn points_op(index: u32) -> Option<SpellModOp> {
        match index {
            0 => Some(SpellModOp::PointsIndex0),
            1 => Some(SpellModOp::PointsIndex1),
            2 => Some(SpellModOp::PointsIndex2),
            3 => Some(SpellModOp::PointsIndex3),
            4 => Some(SpellModOp::PointsIndex4),
            _ => None,
        }
    }

    /// The value of effect `index` of a spell with `class` options after `POINTS` (every effect)
    /// and `POINTS_INDEX_n` modifiers: `(base + flat) × (1 + pct)` with both operations' amounts
    /// combined.
    pub fn effect_value(&self, class: Option<&ClassOptions>, index: u32, base: f64) -> f64 {
        let mut flat = self.flat(class, SpellModOp::Points);
        let mut pct = self.pct(class, SpellModOp::Points);
        if let Some(op) = Self::points_op(index) {
            flat += self.flat(class, op);
            pct += self.pct(class, op);
        }
        (base + flat) * (1.0 + pct / 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spell::dbc::{AuraType, SpellEffectName};

    fn modifier(op: SpellModOp, pct: bool, amount: f64, mask0: u32) -> SpellModifier {
        SpellModifier {
            set: 4,
            class_mask: [mask0, 0, 0, 0],
            op,
            pct,
            amount,
            source: 1,
        }
    }

    const HEROIC_STRIKE: ClassOptions = ClassOptions {
        set: 4,
        mask: [64, 0, 0, 0],
    };

    fn hs() -> Option<&'static ClassOptions> {
        Some(&HEROIC_STRIKE)
    }

    #[test]
    fn modifiers_apply_by_family_and_class_mask() {
        let mut table = SpellModifiers::new();
        table.add(modifier(SpellModOp::PowerCost0, false, -10.0, 64));
        table.add(modifier(SpellModOp::PowerCost0, false, -20.0, 128));
        table.add(modifier(
            SpellModOp::CritDamageAndHealing,
            true,
            20.0,
            64 | 128,
        ));
        assert_eq!(table.flat(hs(), SpellModOp::PowerCost0), -10.0);
        assert_eq!(table.pct(hs(), SpellModOp::CritDamageAndHealing), 20.0);
        assert_eq!(
            table.multiplier(hs(), SpellModOp::CritDamageAndHealing),
            1.2
        );
        assert_eq!(table.apply(hs(), SpellModOp::PowerCost0, 150.0), 140.0);
        assert_eq!(table.apply(hs(), SpellModOp::Cooldown, 5.0), 5.0);
        let other_family = ClassOptions {
            set: 5,
            mask: [64, 0, 0, 0],
        };
        assert_eq!(table.flat(Some(&other_family), SpellModOp::PowerCost0), 0.0);
        assert_eq!(table.flat(None, SpellModOp::PowerCost0), 0.0);
        assert_eq!(table.applicable(hs(), SpellModOp::PowerCost0).count(), 1);
    }

    #[test]
    fn effect_values_combine_points_and_points_index_ops() {
        let mut table = SpellModifiers::new();
        table.add(modifier(SpellModOp::Points, true, 50.0, 64));
        table.add(modifier(SpellModOp::PointsIndex1, false, 10.0, 64));
        assert_eq!(table.effect_value(hs(), 0, 100.0), 150.0);
        assert_eq!(table.effect_value(hs(), 1, 100.0), 165.0);
        assert_eq!(SpellModifiers::points_op(4), Some(SpellModOp::PointsIndex4));
        assert_eq!(SpellModifiers::points_op(5), None);
    }

    #[test]
    fn modifiers_are_removed_one_at_a_time_or_by_source() {
        let mut table = SpellModifiers::new();
        let a = modifier(SpellModOp::Points, true, 5.0, 64);
        table.add(a);
        table.add(a);
        let mut b = modifier(SpellModOp::Points, true, 7.0, 64);
        b.source = 2;
        table.add(b);
        assert_eq!(table.len(), 3);
        assert!(table.remove(&a));
        assert_eq!(table.pct(hs(), SpellModOp::Points), 12.0);
        table.remove_source(2);
        assert_eq!(table.all(), [a]);
        assert!(!table.remove(&b));
        assert!(!table.is_empty());
    }

    #[test]
    fn modifiers_are_built_from_aura_effects() {
        let mut effect = EffectRecord::new(0, SpellEffectName::ApplyAura);
        effect.aura = AuraType::AddPctModifier;
        effect.misc_value = [15, 0];
        effect.spell_class_mask = [64, 8, 0, 0];
        let modifier = SpellModifier::from_effect(&effect, 16493, 4, 20.0);
        assert_eq!(modifier.op, SpellModOp::CritDamageAndHealing);
        assert!(modifier.pct);
        assert_eq!(modifier.amount, 20.0);
        assert_eq!(modifier.class_mask, [64, 8, 0, 0]);
        assert!(modifier.applies_to(hs()));
        effect.aura = AuraType::AddFlatModifier;
        assert!(!SpellModifier::from_effect(&effect, 1, 4, 1.0).pct);
    }
}
