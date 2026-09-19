//! Pruning: drops what a damage simulator has no use for from an exported file
//! (`data/SPELL_INSTRUCTIONS.md` §1.9, `data/spells/overrides/discard.txt`).
//!
//! 1. Effects whose kind or aura is discarded (`csim_engine::spell::dbc::DISCARDED_AURAS` /
//!    `DISCARDED_EFFECTS`: crowd control, movement, immunities, healing, …) are removed from
//!    their spells; the remaining effects keep their table indices.
//! 2. A spell with no effects left is dropped (Taunt: `ATTACK_ME` + `MOD_TAUNT`), and so is
//!    every trigger effect that pointed at a dropped spell (Intimidating Shout's stun), which can
//!    empty further spells — repeated to a fixed point. Action-bar overrides of dropped spells
//!    and `supercedes` links to dropped ranks go the same way.
//!
//! Mocking Blow keeps its `SCHOOL_DAMAGE` and stays; Bloodthirst loses its run-speed aura and
//! stays. Spells the overrides mention — with an entry of their own or through another entry's
//! parameters (Berserker Rage, which Improved Berserker Rage's `GAIN_RESOURCE_ON_USE` names) —
//! are kept even when nothing is left of them: the overrides are the hand-written intent. The
//! report lists what went so the exporter can print it.

use std::collections::{BTreeMap, BTreeSet};

use csim_engine::spell::dbc::AuraType;
use csim_engine::spell::overrides::Overrides;
use csim_engine::spell::record::{EffectRecord, SpellFile};

/// One removed effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedEffect {
    pub spell: u32,
    pub name: String,
    pub index: u32,
    /// What the effect was (`aura MOD_STUN`, `effect ATTACK_ME`, `trigger of 20511`).
    pub what: String,
}

/// One removed spell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedSpell {
    pub id: u32,
    pub name: String,
}

/// What [`prune`] removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub effects: Vec<DroppedEffect>,
    pub spells: Vec<DroppedSpell>,
}

impl PruneReport {
    /// The dropped spells as `"Name (id)"`, sorted by id.
    pub fn spell_names(&self) -> Vec<String> {
        self.spells
            .iter()
            .map(|s| format!("{} ({})", s.name, s.id))
            .collect()
    }
}

fn describe(effect: &EffectRecord) -> String {
    if effect.is_apply_aura() {
        format!("aura {}", effect.aura)
    } else {
        format!("effect {}", effect.effect)
    }
}

/// Removes the discarded effects and the spells that become empty, except those `overrides`
/// mention (see the module docs).
pub fn prune(file: &mut SpellFile, overrides: &Overrides) -> PruneReport {
    let mut report = PruneReport::default();
    let protected: BTreeSet<u32> = overrides
        .all()
        .iter()
        .flat_map(|o| std::iter::once(o.id).chain(o.referenced_spells()))
        .collect();

    // 1. Discarded effects.
    for spell in &mut file.spells {
        let (dropped, kept): (Vec<_>, Vec<_>) = spell
            .effects
            .drain(..)
            .partition(EffectRecord::is_discarded);
        for effect in dropped {
            report.effects.push(DroppedEffect {
                spell: spell.id,
                name: spell.name.clone(),
                index: effect.index,
                what: describe(&effect),
            });
        }
        spell.effects = kept;
    }

    // 2. Empty spells, and the triggers that pointed at them, to a fixed point.
    let mut dropped_ids: BTreeSet<u32> = BTreeSet::new();
    loop {
        let names: BTreeMap<u32, String> =
            file.spells.iter().map(|s| (s.id, s.name.clone())).collect();
        let empty: Vec<u32> = file
            .spells
            .iter()
            .filter(|s| s.effects.is_empty() && !protected.contains(&s.id))
            .map(|s| s.id)
            .collect();
        if empty.is_empty() {
            break;
        }
        for id in &empty {
            report.spells.push(DroppedSpell {
                id: *id,
                name: names[id].clone(),
            });
            dropped_ids.insert(*id);
        }
        file.spells.retain(|s| !empty.contains(&s.id));

        for spell in &mut file.spells {
            let (dropped, kept): (Vec<_>, Vec<_>) = spell.effects.drain(..).partition(|e| {
                let trigger = e.trigger_spell != 0 && dropped_ids.contains(&e.trigger_spell);
                let actionbar = e.is_apply_aura()
                    && e.aura == AuraType::OverrideActionbarSpells
                    && (dropped_ids.contains(&(e.misc_value[0] as u32))
                        || dropped_ids.contains(&(e.base_points as u32)));
                trigger || actionbar
            });
            for effect in dropped {
                report.effects.push(DroppedEffect {
                    spell: spell.id,
                    name: spell.name.clone(),
                    index: effect.index,
                    what: if effect.trigger_spell != 0 {
                        format!("trigger of dropped {}", effect.trigger_spell)
                    } else {
                        format!(
                            "action-bar override of dropped {} / {}",
                            effect.misc_value[0], effect.base_points as u32
                        )
                    },
                });
            }
            spell.effects = kept;
        }
    }

    for spell in &mut file.spells {
        if dropped_ids.contains(&spell.supercedes) {
            spell.supercedes = 0;
        }
    }
    report.spells.sort_by_key(|s| s.id);
    report.effects.sort_by_key(|e| (e.spell, e.index));
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use csim_engine::spell::dbc::{ImplicitTarget, SpellEffectName};
    use csim_engine::spell::record::SpellRecord;

    fn aura(index: u32, aura: AuraType) -> EffectRecord {
        let mut effect = EffectRecord::new(index, SpellEffectName::ApplyAura);
        effect.aura = aura;
        effect.implicit_target = [ImplicitTarget::UnitTargetEnemy, ImplicitTarget::None];
        effect
    }

    fn trigger(index: u32, spell: u32) -> EffectRecord {
        let mut effect = EffectRecord::new(index, SpellEffectName::TriggerSpell);
        effect.trigger_spell = spell;
        effect
    }

    fn file(spells: Vec<SpellRecord>) -> SpellFile {
        SpellFile {
            build: "1.60.1.69893".into(),
            class: None,
            spells,
        }
    }

    #[test]
    fn empty_spells_and_their_triggers_are_dropped_to_a_fixed_point() {
        let mut taunt = SpellRecord::new(355, "Taunt");
        taunt
            .effects
            .push(EffectRecord::new(0, SpellEffectName::AttackMe));
        taunt.effects.push(aura(1, AuraType::ModTaunt));
        let mut mocking = SpellRecord::new(694, "Mocking Blow");
        mocking
            .effects
            .push(EffectRecord::new(0, SpellEffectName::SchoolDamage));
        mocking.effects.push(aura(1, AuraType::ModTaunt));
        let mut stun = SpellRecord::new(20511, "Intimidating Shout");
        stun.effects.push(aura(0, AuraType::ModStun));
        let mut shout = SpellRecord::new(5246, "Intimidating Shout");
        shout.effects.push(trigger(0, 20511));
        shout.effects.push(aura(1, AuraType::ModFear));
        shout.effects.push(aura(2, AuraType::ModIncreaseSpeed));
        let mut talent = SpellRecord::new(12289, "Improved Hamstring");
        let mut proc = aura(0, AuraType::ProcTriggerSpell);
        proc.trigger_spell = 5246;
        talent.effects.push(proc);
        let mut next_rank = SpellRecord::new(356, "Taunt");
        next_rank.supercedes = 355;
        next_rank
            .effects
            .push(EffectRecord::new(0, SpellEffectName::SchoolDamage));

        let mut f = file(vec![taunt, mocking, stun, shout, talent, next_rank]);
        let report = prune(&mut f, &Overrides::new());
        let ids: Vec<u32> = f.spells.iter().map(|s| s.id).collect();
        assert_eq!(ids, [694, 356]);
        assert_eq!(f.spells[0].effects.len(), 1);
        assert_eq!(f.spells[0].effects[0].index, 0);
        assert_eq!(f.spells[1].supercedes, 0, "the dropped rank is unlinked");
        assert_eq!(
            report.spell_names(),
            [
                "Taunt (355)",
                "Intimidating Shout (5246)",
                "Improved Hamstring (12289)",
                "Intimidating Shout (20511)"
            ]
        );
        assert_eq!(report.effects.len(), 8);
        let shout_trigger = report
            .effects
            .iter()
            .find(|e| e.spell == 5246 && e.index == 0)
            .unwrap();
        assert_eq!(shout_trigger.what, "trigger of dropped 20511");
        assert_eq!(report.effects[0].what, "effect ATTACK_ME");
    }

    #[test]
    fn spells_the_overrides_mention_are_kept() {
        let mut rage = SpellRecord::new(18499, "Berserker Rage");
        rage.effects.push(aura(0, AuraType::MechanicImmunity));
        let mut f = file(vec![rage]);
        let mut overrides = Overrides::new();
        let mut talent = csim_engine::spell::overrides::SpellOverride::new(20500);
        talent
            .effects
            .push(csim_engine::spell::overrides::EffectScript {
                index: 0,
                script: csim_engine::spell::overrides::ScriptKind::GainResourceOnUse,
                params: csim_engine::spell::overrides::ScriptParams {
                    spell: Some(18499),
                    resource: Some(csim_engine::spell::dbc::PowerType::Rage),
                    ..Default::default()
                },
            });
        overrides.add(talent).unwrap();
        let report = prune(&mut f, &overrides);
        assert_eq!(f.spells.len(), 1);
        assert!(f.spells[0].effects.is_empty());
        assert!(report.spells.is_empty());
        assert_eq!(report.effects.len(), 1);
    }

    #[test]
    fn kept_effects_keep_their_indices() {
        let mut ms = SpellRecord::new(12294, "Mortal Strike");
        ms.effects.push(aura(0, AuraType::ModHealingPct));
        ms.effects
            .push(EffectRecord::new(1, SpellEffectName::NormalizedWeaponDmg));
        let mut f = file(vec![ms]);
        let report = prune(&mut f, &Overrides::new());
        assert_eq!(f.spells[0].effects.len(), 1);
        assert_eq!(f.spells[0].effects[0].index, 1);
        assert!(report.spells.is_empty());
        assert_eq!(report.effects[0].what, "aura MOD_HEALING_PCT");
    }

    #[test]
    fn action_bar_overrides_of_dropped_spells_go_too() {
        let mut gone = SpellRecord::new(10, "Gone");
        gone.effects.push(aura(0, AuraType::ModStun));
        let mut talent = SpellRecord::new(11, "Talent");
        let mut swap = aura(0, AuraType::OverrideActionbarSpells);
        swap.misc_value = [10, 0];
        swap.base_points = 12.0;
        talent.effects.push(swap);
        let mut f = file(vec![gone, talent]);
        let report = prune(&mut f, &Overrides::new());
        assert!(f.spells.is_empty());
        assert_eq!(report.spells.len(), 2);
    }
}
