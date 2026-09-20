//! `SpellEffect.Effect`: what an effect row does.

use super::dbc_enum;

dbc_enum! {
    /// Retail `SpellEffectName`, the `Effect` column of `SpellEffect`.
    ///
    /// The engine interprets the values listed in `data/SPELL_INSTRUCTIONS.md` §1.7; the rest are
    /// named so that reports read well and are otherwise treated as no-ops. The values a damage
    /// simulator has no use for are not named at all (`super::discard`): they load as
    /// `Unknown(id)` and the exporter drops them.
    SpellEffectName: u32 {
        None = 0 => "NONE",
        Instakill = 1 => "INSTAKILL",
        /// Flat damage; the hit table comes from `SpellCategories.DefenseType`.
        SchoolDamage = 2 => "SCHOOL_DAMAGE",
        /// Scripted (Bloodthirst's AP part, Execute, Deep Wounds' bleed).
        Dummy = 3 => "DUMMY",
        PortalTeleport = 4 => "PORTAL_TELEPORT",
        /// Applies the aura in `EffectAura`.
        ApplyAura = 6 => "APPLY_AURA",
        EnvironmentalDamage = 7 => "ENVIRONMENTAL_DAMAGE",
        PowerDrain = 8 => "POWER_DRAIN",
        HealthLeech = 9 => "HEALTH_LEECH",
        Heal = 10 => "HEAL",
        Bind = 11 => "BIND",
        Portal = 12 => "PORTAL",
        QuestComplete = 16 => "QUEST_COMPLETE",
        /// Weapon damage + base points, no school conversion (Heroic Strike, Cleave, Slam).
        WeaponDamageNoschool = 17 => "WEAPON_DAMAGE_NOSCHOOL",
        Resurrect = 18 => "RESURRECT",
        /// Extra attacks (Sword Specialization payload).
        AddExtraAttacks = 19 => "ADD_EXTRA_ATTACKS",
        Dodge = 20 => "DODGE",
        Evade = 21 => "EVADE",
        Parry = 22 => "PARRY",
        Block = 23 => "BLOCK",
        CreateItem = 24 => "CREATE_ITEM",
        Weapon = 25 => "WEAPON",
        Defense = 26 => "DEFENSE",
        PersistentAreaAura = 27 => "PERSISTENT_AREA_AURA",
        Summon = 28 => "SUMMON",
        Leap = 29 => "LEAP",
        /// Resource gain, misc = power type; rage in tenths (Unbridled Wrath 10 = 1 rage).
        Energize = 30 => "ENERGIZE",
        /// Base points % of weapon damage (Raging Blow, Spearing Strike).
        WeaponPercentDamage = 31 => "WEAPON_PERCENT_DAMAGE",
        TriggerMissile = 32 => "TRIGGER_MISSILE",
        OpenLock = 33 => "OPEN_LOCK",
        SummonChangeItem = 34 => "SUMMON_CHANGE_ITEM",
        ApplyAreaAuraParty = 35 => "APPLY_AREA_AURA_PARTY",
        LearnSpell = 36 => "LEARN_SPELL",
        SpellDefense = 37 => "SPELL_DEFENSE",
        Language = 39 => "LANGUAGE",
        DualWield = 40 => "DUAL_WIELD",
        Jump = 41 => "JUMP",
        JumpDest = 42 => "JUMP_DEST",
        TeleportUnitsFaceCaster = 43 => "TELEPORT_UNITS_FACE_CASTER",
        SkillStep = 44 => "SKILL_STEP",
        PlayMovie = 45 => "PLAY_MOVIE",
        Spawn = 46 => "SPAWN",
        TradeSkill = 47 => "TRADE_SKILL",
        Stealth = 48 => "STEALTH",
        Detect = 49 => "DETECT",
        TransDoor = 50 => "TRANS_DOOR",
        ForceCriticalHit = 51 => "FORCE_CRITICAL_HIT",
        EnchantItem = 53 => "ENCHANT_ITEM",
        EnchantItemTemporary = 54 => "ENCHANT_ITEM_TEMPORARY",
        Tamecreature = 55 => "TAMECREATURE",
        SummonPet = 56 => "SUMMON_PET",
        LearnPetSpell = 57 => "LEARN_PET_SPELL",
        /// Weapon damage + base points in the spell's school.
        WeaponDamage = 58 => "WEAPON_DAMAGE",
        CreateRandomItem = 59 => "CREATE_RANDOM_ITEM",
        Proficiency = 60 => "PROFICIENCY",
        SendEvent = 61 => "SEND_EVENT",
        PowerBurn = 62 => "POWER_BURN",
        /// Flat threat (Sunder Armor).
        Threat = 63 => "THREAT",
        /// Casts `EffectTriggerSpell` (Bloodrage's periodic part, Execute's marker).
        TriggerSpell = 64 => "TRIGGER_SPELL",
        ApplyAreaAuraRaid = 65 => "APPLY_AREA_AURA_RAID",
        CreateManaGem = 66 => "CREATE_MANA_GEM",
        HealMaxHealth = 67 => "HEAL_MAX_HEALTH",
        Distract = 69 => "DISTRACT",
        Pickpocket = 71 => "PICKPOCKET",
        AddFarsight = 72 => "ADD_FARSIGHT",
        UntrainTalents = 73 => "UNTRAIN_TALENTS",
        ApplyGlyph = 74 => "APPLY_GLYPH",
        HealMechanical = 75 => "HEAL_MECHANICAL",
        SummonObjectWild = 76 => "SUMMON_OBJECT_WILD",
        ScriptEffect = 77 => "SCRIPT_EFFECT",
        Attack = 78 => "ATTACK",
        Sanctuary = 79 => "SANCTUARY",
        BindSight = 82 => "BIND_SIGHT",
        Duel = 83 => "DUEL",
        Stuck = 84 => "STUCK",
        SummonPlayer = 85 => "SUMMON_PLAYER",
        ActivateObject = 86 => "ACTIVATE_OBJECT",
        GameobjectDamage = 87 => "GAMEOBJECT_DAMAGE",
        GameobjectRepair = 88 => "GAMEOBJECT_REPAIR",
        GameobjectSetDestructionState = 89 => "GAMEOBJECT_SET_DESTRUCTION_STATE",
        KillCredit = 90 => "KILL_CREDIT",
        ThreatAll = 91 => "THREAT_ALL",
        EnchantHeldItem = 92 => "ENCHANT_HELD_ITEM",
        ForceDeselect = 93 => "FORCE_DESELECT",
        SelfResurrect = 94 => "SELF_RESURRECT",
        Skinning = 95 => "SKINNING",
        Charge = 96 => "CHARGE",
        CastButton = 97 => "CAST_BUTTON",
        KnockBack = 98 => "KNOCK_BACK",
        Disenchant = 99 => "DISENCHANT",
        Inebriate = 100 => "INEBRIATE",
        FeedPet = 101 => "FEED_PET",
        DismissPet = 102 => "DISMISS_PET",
        Reputation = 103 => "REPUTATION",
        SummonObjectSlot1 = 104 => "SUMMON_OBJECT_SLOT1",
        Survey = 105 => "SURVEY",
        ChangeRaidMarker = 106 => "CHANGE_RAID_MARKER",
        ShowCorpseLoot = 107 => "SHOW_CORPSE_LOOT",
        ResurrectPet = 109 => "RESURRECT_PET",
        DestroyAllTotems = 110 => "DESTROY_ALL_TOTEMS",
        DurabilityDamage = 111 => "DURABILITY_DAMAGE",
        ResurrectNew = 113 => "RESURRECT_NEW",
        DurabilityDamagePct = 115 => "DURABILITY_DAMAGE_PCT",
        SkinPlayerCorpse = 116 => "SKIN_PLAYER_CORPSE",
        SpiritHeal = 117 => "SPIRIT_HEAL",
        Skill = 118 => "SKILL",
        ApplyAreaAuraPet = 119 => "APPLY_AREA_AURA_PET",
        TeleportGraveyard = 120 => "TELEPORT_GRAVEYARD",
        /// Normalized weapon damage + base points (Mortal Strike, Whirlwind, Overpower).
        NormalizedWeaponDmg = 121 => "NORMALIZED_WEAPON_DMG",
        SendTaxi = 123 => "SEND_TAXI",
        PullTowards = 124 => "PULL_TOWARDS",
        ModifyThreatPercent = 125 => "MODIFY_THREAT_PERCENT",
        StealBeneficialBuff = 126 => "STEAL_BENEFICIAL_BUFF",
        Prospecting = 127 => "PROSPECTING",
        ApplyAreaAuraFriend = 128 => "APPLY_AREA_AURA_FRIEND",
        ApplyAreaAuraEnemy = 129 => "APPLY_AREA_AURA_ENEMY",
        RedirectThreat = 130 => "REDIRECT_THREAT",
        PlaySound = 131 => "PLAY_SOUND",
        PlayMusic = 132 => "PLAY_MUSIC",
        UnlearnSpecialization = 133 => "UNLEARN_SPECIALIZATION",
        KillCredit2 = 134 => "KILL_CREDIT2",
        CallPet = 135 => "CALL_PET",
        HealPct = 136 => "HEAL_PCT",
        EnergizePct = 137 => "ENERGIZE_PCT",
        LeapBack = 138 => "LEAP_BACK",
        ClearQuest = 139 => "CLEAR_QUEST",
        ForceCast = 140 => "FORCE_CAST",
        ForceCastWithValue = 141 => "FORCE_CAST_WITH_VALUE",
        TriggerSpellWithValue = 142 => "TRIGGER_SPELL_WITH_VALUE",
        ApplyAreaAuraOwner = 143 => "APPLY_AREA_AURA_OWNER",
        KnockBackDest = 144 => "KNOCK_BACK_DEST",
        PullTowardsDest = 145 => "PULL_TOWARDS_DEST",
        QuestFail = 147 => "QUEST_FAIL",
        TriggerMissileSpellWithValue = 148 => "TRIGGER_MISSILE_SPELL_WITH_VALUE",
        ChargeDest = 149 => "CHARGE_DEST",
        QuestStart = 150 => "QUEST_START",
        TriggerSpell2 = 151 => "TRIGGER_SPELL_2",
        SummonRafFriend = 152 => "SUMMON_RAF_FRIEND",
        CreateTamedPet = 153 => "CREATE_TAMED_PET",
        DiscoverTaxi = 154 => "DISCOVER_TAXI",
        TitanGrip = 155 => "TITAN_GRIP",
        EnchantItemPrismatic = 156 => "ENCHANT_ITEM_PRISMATIC",
        CreateLoot = 157 => "CREATE_LOOT",
        Milling = 158 => "MILLING",
        AllowRenamePet = 159 => "ALLOW_RENAME_PET",
        ForceCast2 = 160 => "FORCE_CAST_2",
        TalentSpecCount = 161 => "TALENT_SPEC_COUNT",
        TalentSpecSelect = 162 => "TALENT_SPEC_SELECT",
        ObliterateItem = 163 => "OBLITERATE_ITEM",
        RemoveAura = 164 => "REMOVE_AURA",
        DamageFromMaxHealthPct = 165 => "DAMAGE_FROM_MAX_HEALTH_PCT",
        GiveCurrency = 166 => "GIVE_CURRENCY",
        AllowControlPet = 168 => "ALLOW_CONTROL_PET",
        DestroyItem = 169 => "DESTROY_ITEM",
        UpdateZoneAurasAndPhases = 170 => "UPDATE_ZONE_AURAS_AND_PHASES",
        SummonPersonalGameobject = 171 => "SUMMON_PERSONAL_GAMEOBJECT",
        ResurrectWithAura = 172 => "RESURRECT_WITH_AURA",
        UnlockGuildVaultTab = 173 => "UNLOCK_GUILD_VAULT_TAB",
        ApplyAuraOnPet = 174 => "APPLY_AURA_ON_PET",
        Sanctuary2 = 176 => "SANCTUARY_2",
        DespawnPersistentAreaAura = 177 => "DESPAWN_PERSISTENT_AREA_AURA",
        CreateAreatrigger = 179 => "CREATE_AREATRIGGER",
        UpdateAreatrigger = 180 => "UPDATE_AREATRIGGER",
        RemoveTalent = 181 => "REMOVE_TALENT",
        DespawnAreatrigger = 182 => "DESPAWN_AREATRIGGER",
        Reputation2 = 184 => "REPUTATION_2",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_names_round_trip() {
        for effect in SpellEffectName::KNOWN {
            assert_eq!(SpellEffectName::from_id(effect.id()), *effect);
            assert_eq!(
                SpellEffectName::from_name(effect.name().unwrap()),
                Some(*effect)
            );
            assert_eq!(
                effect.to_string().parse::<SpellEffectName>().unwrap(),
                *effect
            );
        }
        let mut ids: Vec<u32> = SpellEffectName::KNOWN.iter().map(|e| e.id()).collect();
        ids.dedup();
        assert_eq!(
            ids.len(),
            SpellEffectName::KNOWN.len(),
            "ids are unique and ascending"
        );
        assert!(ids.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn values_seen_on_player_spells_have_the_documented_names() {
        let seen = [
            (2, "SCHOOL_DAMAGE"),
            (3, "DUMMY"),
            (6, "APPLY_AURA"),
            (10, "HEAL"),
            (17, "WEAPON_DAMAGE_NOSCHOOL"),
            (19, "ADD_EXTRA_ATTACKS"),
            (24, "CREATE_ITEM"),
            (27, "PERSISTENT_AREA_AURA"),
            (28, "SUMMON"),
            (30, "ENERGIZE"),
            (31, "WEAPON_PERCENT_DAMAGE"),
            (35, "APPLY_AREA_AURA_PARTY"),
            (36, "LEARN_SPELL"),
            (54, "ENCHANT_ITEM_TEMPORARY"),
            (58, "WEAPON_DAMAGE"),
            (63, "THREAT"),
            (64, "TRIGGER_SPELL"),
            (65, "APPLY_AREA_AURA_RAID"),
            (77, "SCRIPT_EFFECT"),
            (96, "CHARGE"),
            (98, "KNOCK_BACK"),
            (121, "NORMALIZED_WEAPON_DMG"),
        ];
        for (id, name) in seen {
            assert_eq!(SpellEffectName::from_id(id).name(), Some(name), "{id}");
        }
        assert_eq!(
            SpellEffectName::from_id(9999),
            SpellEffectName::Unknown(9999)
        );
    }

    #[test]
    fn serde_uses_names() {
        assert_eq!(
            serde_yaml::from_str::<SpellEffectName>("NORMALIZED_WEAPON_DMG").unwrap(),
            SpellEffectName::NormalizedWeaponDmg
        );
        assert_eq!(
            serde_yaml::from_str::<SpellEffectName>("121").unwrap(),
            SpellEffectName::NormalizedWeaponDmg
        );
        assert_eq!(
            serde_yaml::to_string(&SpellEffectName::Threat)
                .unwrap()
                .trim(),
            "THREAT"
        );
    }
}
