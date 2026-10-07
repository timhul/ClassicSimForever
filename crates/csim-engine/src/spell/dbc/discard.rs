//! The auras and effects a damage simulator has no use for (`data/spells/overrides/discard.txt`).
//!
//! Crowd control, movement, immunities, healing, phasing, mounts, … change nothing in a
//! single-target damage simulation, so the vocabulary in [`aura.rs`](super::AuraType) and
//! [`effect.rs`](super::SpellEffectName) does not name them: they load as `Unknown(id)`. The ids
//! are listed here so the exporter can tell a discarded value from a genuinely new one: it
//! drops discarded effects from the records it writes (`csim-tables export-spells`,
//! `data/SPELL_INSTRUCTIONS.md` §1.10), and a spell whose effects are all dropped — or that
//! only triggers dropped spells — is left out of the data files altogether (Taunt: `ATTACK_ME`
//! and `MOD_TAUNT`; Mocking Blow keeps its `SCHOOL_DAMAGE`). The engine's "unsupported" report
//! skips them and flags every other unknown id.
//!
//! Kept although `discard.txt` lists them as candidates: `MOD_THREAT` and `MOD_TOTAL_THREAT`
//! (the simulator tracks threat: stance passives, Defiance) and `OVERRIDE_ACTIONBAR_SPELLS`
//! (Improved Slam's and the runes' rank replacement), `ADD_TARGET_TRIGGER` (Relentless
//! Strikes' energy on finishers), `MOD_DAMAGE_TAKEN` (Judgement of the Crusader's holy damage
//! taken) and `MOD_MANA_REGEN_INTERRUPT` (Reverence's mana regeneration while casting).

use super::{AuraType, SpellEffectName};

/// `SpellEffect.EffectAura` values (of `APPLY_AURA` effects) that are discarded.
pub const DISCARDED_AURA_IDS: &[u32] = &[
    1,   // BIND_SIGHT
    2,   // MOD_POSSESS
    5,   // MOD_CONFUSE
    6,   // MOD_CHARM
    7,   // MOD_FEAR
    8,   // PERIODIC_HEAL
    11,  // MOD_TAUNT
    12,  // MOD_STUN
    15,  // DAMAGE_SHIELD
    17,  // MOD_STEALTH_DETECT
    18,  // MOD_INVISIBILITY
    19,  // MOD_INVISIBILITY_DETECT
    25,  // MOD_PACIFY
    26,  // MOD_ROOT
    27,  // MOD_SILENCE
    28,  // REFLECT_SPELLS
    31,  // MOD_INCREASE_SPEED
    32,  // MOD_INCREASE_MOUNTED_SPEED
    33,  // MOD_DECREASE_SPEED
    37,  // EFFECT_IMMUNITY
    38,  // STATE_IMMUNITY
    39,  // SCHOOL_IMMUNITY
    40,  // DAMAGE_IMMUNITY
    41,  // DISPEL_IMMUNITY
    44,  // TRACK_CREATURES
    45,  // TRACK_RESOURCES
    50,  // MOD_CRITICAL_HEALING_AMOUNT
    58,  // MOD_INCREASE_SWIM_SPEED
    60,  // MOD_PACIFY_SILENCE
    61,  // MOD_SCALE
    66,  // FEIGN_DEATH
    67,  // MOD_DISARM
    68,  // MOD_STALKED
    69,  // SCHOOL_ABSORB
    74,  // REFLECT_SPELLS_SCHOOL
    75,  // MOD_LANGUAGE
    76,  // FAR_SIGHT
    77,  // MECHANIC_IMMUNITY
    78,  // MOUNTED
    81,  // SPLIT_DAMAGE_PCT
    82,  // WATER_BREATHING
    84,  // MOD_REGEN
    86,  // CHANNEL_DEATH_ITEM
    88,  // MOD_HEALTH_REGEN_PERCENT
    91,  // MOD_DETECT_RANGE
    92,  // PREVENTS_FLEEING
    93,  // MOD_UNATTACKABLE
    94,  // INTERRUPT_REGEN
    95,  // GHOST
    96,  // SPELL_MAGNET
    97,  // MANA_SHIELD
    100, // AURAS_VISIBLE
    104, // WATER_WALK
    105, // FEATHER_FALL
    106, // HOVER
    111, // INTERCEPT_MELEE_RANGED_ATTACKS
    112, // OVERRIDE_CLASS_SCRIPTS
    115, // MOD_HEALING
    116, // MOD_REGEN_DURING_COMBAT
    117, // MOD_MECHANIC_RESISTANCE
    118, // MOD_HEALING_PCT
    120, // UNTRACKABLE
    121, // EMPATHY
    128, // MOD_POSSESS_PET
    129, // MOD_SPEED_ALWAYS
    130, // MOD_MOUNTED_SPEED_ALWAYS
    135, // MOD_HEALING_DONE
    136, // MOD_HEALING_DONE_PERCENT
    139, // FORCE_REACTION
    143, // MOD_RESISTANCE_EXCLUSIVE
    148, // MOD_CHARGE_RECOVERY_RATE
    149, // REDUCE_PUSHBACK
    151, // TRACK_STEALTHED
    152, // MOD_DETECTED_RANGE
    154, // MOD_STEALTH_LEVEL
    155, // MOD_WATER_BREATHING
    156, // MOD_REPUTATION_GAIN
    158, // ALLOW_TALENT_SWAPPING
    159, // NO_PVP_CREDIT
    161, // MOD_HEALTH_REGEN_IN_COMBAT
    162, // POWER_BURN
    169, // SET_FFA_PVP
    170, // DETECT_AMORE
    171, // MOD_SPEED_NOT_STACK
    172, // MOD_MOUNTED_SPEED_NOT_STACK
    176, // SPIRIT_OF_REDEMPTION
    177, // AOE_CHARM
    179, // MOD_POWER_DISPLAY
    183, // MOD_CRITICAL_THREAT
    189, // MOD_RATING
    190, // MOD_FACTION_REPUTATION_GAIN
    191, // USE_NORMAL_MOVEMENT_SPEED
    193, // MELEE_SLOW
    200, // MOD_XP_PCT
    201, // FLY
    206, // MOD_INCREASE_VEHICLE_FLIGHT_SPEED
    207, // MOD_INCREASE_MOUNTED_FLIGHT_SPEED
    208, // MOD_INCREASE_FLIGHT_SPEED
    209, // MOD_MOUNTED_FLIGHT_SPEED_ALWAYS
    210, // MOD_VEHICLE_SPEED_ALWAYS
    211, // MOD_FLIGHT_SPEED_NOT_STACK
    215, // ARENA_PREPARATION
    221, // MOD_DETAUNT
    222, // REMOVE_TRANSMOG_COST
    225, // PREVENT_REGENERATE_POWER
    228, // DETECT_STEALTH
    229, // MOD_AOE_DAMAGE_AVOIDANCE
    232, // MOD_MECHANIC_DURATION
    234, // MOD_MECHANIC_DURATION_NOT_STACK
    235, // MOD_DISPEL_RESIST
    236, // CONTROL_VEHICLE
    238, // MOD_SPELL_HEALING_OF_ATTACK_POWER
    239, // MOD_SCALE_2
    241, // FORCE_MOVE_FORWARD
    243, // MOD_FACTION
    244, // COMPREHEND_LANGUAGE
    245, // MOD_AURA_DURATION_BY_DISPEL
    246, // MOD_AURA_DURATION_BY_DISPEL_NOT_STACK
    247, // CLONE_CASTER
    248, // MOD_COMBAT_RESULT_CHANCE
    249, // CONVERT_RUNE
    252, // MOD_SPEED_SLOW_ALL
    254, // MOD_DISARM_OFFHAND
    256, // NO_REAGENT_USE
    258, // OVERRIDE_SUMMONED_OBJECT
    259, // MOD_HOT_PCT
    260, // SCREEN_EFFECT
    261, // PHASE
    262, // ABILITY_IGNORE_AURASTATE
    263, // DISABLE_CASTING_EXCEPT_ABILITIES
    264, // DISABLE_ATTACKING_EXCEPT_ABILITIES
    266, // SET_VIGNETTE
    267, // MOD_IMMUNE_AURA_APPLY_SCHOOL
    273, // X_RAY
    276, // MOD_DAMAGE_DONE_FOR_MECHANIC
    277, // MOD_MAX_AFFECTED_TARGETS
    278, // MOD_DISARM_RANGED
    279, // INITIALIZE_IMAGES
    281, // MOD_HONOR_GAIN_PCT
    283, // MOD_HEALING_RECEIVED
    284, // LINKED
    285, // LINKED_2
    286, // MOD_RECOVERY_RATE
    287, // DEFLECT_SPELLS
    288, // IGNORE_HIT_DIRECTION
    289, // PREVENT_DURABILITY_LOSS
    291, // MOD_XP_QUEST_PCT
    292, // OPEN_STABLE
    293, // OVERRIDE_SPELLS
    294, // PREVENT_REGENERATE_POWER_2
    295, // MOD_PERIODIC_DAMAGE_TAKEN
    300, // SHARE_DAMAGE_PCT
    301, // SCHOOL_HEAL_ABSORB
    304, // MOD_FAKE_INEBRIATE
    305, // MOD_MINIMUM_SPEED
    307, // CAST_WHILE_WALKING_BY_SPELL_LABEL
    309, // MOD_RESILIENCE
    310, // MOD_CREATURE_AOE_DAMAGE_AVOIDANCE
    311, // IGNORE_COMBAT
    312, // ANIM_REPLACEMENT_SET
    314, // PREVENT_RESURRECTION
    315, // UNDERWATER_WALKING
    316, // SCHOOL_ABSORB_OVERKILL
    318, // MASTERY
    321, // MOD_NO_ACTIONS
    322, // INTERFERE_TARGETTING
    326, // PHASE_GROUP
    327, // PHASE_ALWAYS_VISIBLE
    330, // CAST_WHILE_WALKING
    331, // FORCE_WEATHER
    336, // MOUNT_RESTRICTIONS
    337, // MOD_VENDOR_ITEMS_PRICES
    338, // MOD_DURABILITY_LOSS
    340, // MOD_RESURRECTED_HEALTH_BY_GUILD_MEMBER
    343, // MOD_MELEE_DAMAGE_FROM_CASTER
    345, // BYPASS_ARMOR_FOR_CASTER
    348, // MOD_MONEY_GAIN
    349, // MOD_CURRENCY_GAIN
];

/// `SpellEffect.Effect` values that are discarded.
pub const DISCARDED_EFFECT_IDS: &[u32] = &[
    38,  // DISPEL
    68,  // INTERRUPT_CAST
    108, // DISPEL_MECHANIC
    114, // ATTACK_ME
    167, // UPDATE_PLAYER_PHASE
];

impl AuraType {
    /// Whether an aura of this type is dropped from the data (see the module docs).
    pub fn is_discarded(self) -> bool {
        DISCARDED_AURA_IDS.contains(&self.id())
    }
}

impl SpellEffectName {
    /// Whether an effect of this kind is dropped from the data (see the module docs).
    pub fn is_discarded(self) -> bool {
        DISCARDED_EFFECT_IDS.contains(&self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discarded_values_are_not_in_the_vocabulary() {
        for &id in DISCARDED_AURA_IDS {
            let aura = AuraType::from_id(id);
            assert!(!aura.is_known(), "aura {id} is still named");
            assert!(aura.is_discarded());
        }
        for &id in DISCARDED_EFFECT_IDS {
            let effect = SpellEffectName::from_id(id);
            assert!(!effect.is_known(), "effect {id} is still named");
            assert!(effect.is_discarded());
        }
        assert!(AuraType::from_id(12).is_discarded(), "MOD_STUN");
        assert!(AuraType::from_id(118).is_discarded(), "MOD_HEALING_PCT");
        assert!(SpellEffectName::from_id(114).is_discarded(), "ATTACK_ME");
        assert!(
            !AuraType::from_id(999).is_discarded(),
            "unknown, not discarded"
        );
    }

    #[test]
    fn damage_threat_and_overrides_are_kept() {
        assert!(!AuraType::ModThreat.is_discarded());
        assert!(!AuraType::ModTotalThreat.is_discarded());
        assert!(!AuraType::OverrideActionbarSpells.is_discarded());
        assert!(!AuraType::AddTargetTrigger.is_discarded());
        assert!(!AuraType::ModAttackPower.is_discarded());
        assert!(!AuraType::PeriodicDamage.is_discarded());
        assert!(!AuraType::Dummy.is_discarded());
        assert!(!SpellEffectName::SchoolDamage.is_discarded());
        assert!(!SpellEffectName::Dummy.is_discarded());
        assert!(!SpellEffectName::TriggerSpell.is_discarded());
        assert!(!SpellEffectName::ApplyAura.is_discarded());
    }

    #[test]
    fn the_lists_are_sorted_and_unique() {
        for list in [DISCARDED_AURA_IDS, DISCARDED_EFFECT_IDS] {
            assert!(list.windows(2).all(|w| w[0] < w[1]));
        }
    }
}
