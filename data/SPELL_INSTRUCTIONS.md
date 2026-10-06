# 1. Playable class spells (`SkillLine*`, `Trait*`, `Spell*` tables)

Goal: for a playable class, find every spell it can have (spellbook, talents, runes, racials) and
what each spell does, from the dumped build `1.60.1.70009`. Verified against Warrior; the other
classes use the same tables. NPC/creature spells are excluded by only ever walking *from* a class
(sections 1.3–1.6) — never by scanning `SpellName`.

## 1.1 Caveats about this dump

- **Forever re-tunes spells.** Values differ from Classic (Heroic Strike r9 +157 vs +138,
  Bloodthirst 35 % AP + 30, Battle Shout r7 +139 for 3 min, Whirlwind on a 10 s cooldown, Mortal
  Strike weapon + 85). Caster nukes carry `EffectRealPointsPerLevel`, which Classic did not. None
  of the numbers can be validated against ClassicSim data; treat the tables as the truth and
  spot-check in game.
- **The `Talent` / `TalentTab` tables are legacy and stale.** Talents live in the retail *Trait*
  system (section 1.4). `Talent` still lists vanilla layouts, but 124 of the rank spells it points
  at no longer exist anywhere. Do not use it.
- **`SkillLineAbility` has stale rows.** Skip any row whose `Spell` has no `SpellName` row
  (removed talent ranks etc.).
- Coverage looks complete: `SpellName` 31 767, `Spell` 31 767, `SpellMisc` 31 637, `SpellEffect`
  covers 31 754 spells. Only `SpellClassOptions` (6 030), `SpellPower` (3 428), `SpellCooldowns`
  (4 438), `SpellAuraOptions` (12 900), `SpellLevels` (12 806) are sparse by nature — a missing row
  means "no cost / no cooldown / no proc / no level requirement".
- Spell IDs: Classic ≤ ~30 000; SoD-era runes/abilities 400 000–470 000; Forever-only spells
  ≥ 1 200 000.
- `SpellEffect.DifficultyID` is 0 for every player spell; ignore the 13 rows that are not.

## 1.2 Table roles and join keys

```
ChrClasses.ID ──> SkillRaceClassInfo.ClassMask (bit 1<<(ID-1)) ──> SkillLine.ID (CategoryID 7)
                                                                        ├─> SkillLineAbility.SkillLine ──> .Spell   (spellbook, rank chains via .SupercedesSpell)
                                                                        └─> SkillLineXTraitTree.SkillLineID ──> TraitTree.ID   (talents)
ChrClasses.SpellClassSet ──> SpellClassOptions.SpellClassSet            (every spell of the class "family", incl. hidden procs)
ChrRaces.ID ──> SkillLineAbility.RaceMasks_0 (bit 1<<(ID-1)) on the "<Race> Racial" skill lines (CategoryID 9)

TraitTree ──> TraitNode(.TraitTreeID) ──> TraitNodeXTraitNodeEntry ──> TraitNodeEntry(.MaxRanks) ──> TraitDefinition.SpellID
          ──> TraitNodeGroup / TraitNodeGroupXTraitNode / TraitNodeGroupXTraitCond ──> TraitCond      (tier gating)
          ──> TraitEdge(Left → Right)                                                                  (prerequisites)
          ──> TraitDefinitionEffectPoints(.TraitDefinitionID) ──> CurvePoint(.CurveID)                 (value per rank)

Engrave spell (SkillLine 2851) ──> SpellEffect.Effect 54, .EffectMiscValue_0 ──> SpellItemEnchantment.ID ──> .Effect_i 3, .EffectArg_i = rune spell

SpellName.ID = Spell.ID = SpellMisc.SpellID = SpellEffect.SpellID = SpellPower.SpellID = SpellCooldowns.SpellID
             = SpellLevels.SpellID = SpellAuraOptions.SpellID = SpellClassOptions.SpellID = SpellLabel.SpellID
SpellMisc.DurationIndex ──> SpellDuration.ID          SpellAuraOptions.SpellProcsPerMinuteID ──> SpellProcsPerMinute.ID
SpellMisc.CastingTimeIndex / RangeIndex, SpellEffect.EffectRadiusIndex ──> SpellCastTimes / SpellRange / SpellRadius
SpellCategories / SpellShapeshift / SpellAuraRestrictions / SpellEquippedItems / SpellTargetRestrictions / SpellInterrupts / SpellTotems / SpellReagents  (.SpellID, sparse)
SpellEffect.EffectTriggerSpell, EffectAura 42/23 ──> another SpellID          SpellEffect.EffectMiscValue_0 (aura 36) ──> SpellShapeshiftForm.ID
```

| Table | Rows | Use |
|---|---|---|
| `SkillLine` | 154 | Skill lines. `CategoryID` 7 = class skills (3 per class + `Engraving` 2851 + `Runes` 2853 + pet lines), 9 = racials/secondary, 6 = weapon skills. Key `ID`, name `DisplayName_lang` (columns are not in ID-first order). |
| `SkillRaceClassInfo` | 186 | Which classes/races own a skill line (`ClassMask`, `RaceMasks_0`). |
| `SkillLineAbility` | 7 824 | Skill line → spell. `ClassMask`, `RaceMasks_0/1`, `SupercedesSpell` (previous rank), `AcquireMethod`, `MinSkillLineRank`. |
| `SkillLineXTraitTree` | 9 | Class skill line → talent `TraitTree`. |
| `Trait*` (13 tables) | | Talent trees (1.4). |
| `SpellName` | 31 767 | `Name_lang`. Existence test for "spell is real". |
| `Spell` | 31 767 | `NameSubtext_lang` ("Rank 7"), `Description_lang`, `AuraDescription_lang`. Tooltip text with `$` tokens. |
| `SpellMisc` | 31 637 | `Attributes_0..16` (SpellAttr0..16 bit flags), `SchoolMask`, `CastingTimeIndex`, `DurationIndex`, `RangeIndex`, `Speed` (missile). |
| `SpellEffect` | 42 449 | **What the spell does** — one row per effect (1.7). |
| `SpellPower` | 3 428 | Resource cost: `PowerType`, `ManaCost`, `PowerCostPct`, `ManaPerSecond`, `OptionalCost`. |
| `SpellCooldowns` | 4 438 | `RecoveryTime` (spell cd), `CategoryRecoveryTime` (shared cd), `StartRecoveryTime` (GCD, 1500). All ms. |
| `SpellDuration` | 133 | `Duration` ms (−1 = permanent), `MaxDuration`, `DurationPerResource` (combo points). |
| `SpellLevels` | 12 806 | `SpellLevel` (used for per-level scaling), `BaseLevel` (learn level), `MaxLevel` (scaling cap). |
| `SpellAuraOptions` | 12 900 | Procs and stacking: `ProcChance`, `ProcCharges`, `ProcCategoryRecovery` (ICD ms), `ProcTypeMask_0/1`, `SpellProcsPerMinuteID`, `CumulativeAura` (max stacks). |
| `SpellProcsPerMinute` | 11 | `BaseProcRate` PPM (1–10, 2.3). |
| `SpellClassOptions` | 6 030 | `SpellClassSet` (class family) + `SpellClassMask_0..3` (128-bit family flags) — the key for talent/set-bonus modifiers. |
| `SpellCategories` | 10 570 | Per spell: `Category` (shared cooldown group, e.g. 971 = Mortal Strike/Bloodthirst/Shield Slam), `StartRecoveryCategory` (133 "Global" = on the GCD), `DefenseType` (0 none, 1 magic, 2 melee, 3 ranged → which hit table), `Mechanic` (15 bleed …), `DispelType`, `ChargeCategory`, `PreventionType`. |
| `SpellCategory` | 261 | The category definitions (`Name_lang`, `MaxCharges`, `ChargeRecoveryTime`). |
| `SpellShapeshift` | 3 470 | Stance/form requirement: `ShapeshiftMask_0` bit `1 << (SpellShapeshiftForm.ID − 1)` (Battle 65536, Defensive 131072, Berserker 262144; Execute 327680 = Battle+Berserker), `ShapeshiftExclude_0`. |
| `SpellAuraRestrictions` | 317 | Aura-state gates: `TargetAuraState` 2 = target ≤ 20 % (Execute), `CasterAuraState` 1 = after block/dodge/parry (Revenge); `Caster/TargetAuraSpell` = requires that aura. |
| `SpellEquippedItems` | 1 890 | Required weapon: `EquippedItemClass` 2/4, `EquippedItemSubclass` bitmask over `ItemSubClass.SubClassID` (173555 = any melee weapon; class 4 + 64 = shield), `EquippedItemInvTypes`. |
| `SpellTargetRestrictions` | 4 447 | `MaxTargets` (Whirlwind 4), `ConeDegrees`, `TargetCreatureType`. |
| `SpellCastTimes` | 72 | `CastingTimeIndex` → `Base` ms (1 = instant, 16 = 1500). |
| `SpellRange` | 57 | `RangeIndex` → `RangeMax_0` yd (1 self, 2 melee 5 yd, 4 = 30, 5 = 40). |
| `SpellRadius` | 53 | `EffectRadiusIndex` → `Radius` yd (14 = 8, 9 = 20). |
| `SpellInterrupts` | 9 857 | `InterruptFlags` (cast interrupted by movement etc.), `AuraInterruptFlags`, `ChannelInterruptFlags`. |
| `Curve`, `CurvePoint` | 17 198 / 36 914 | Talent rank curves: `CurvePoint.CurveID`, `Pos_0` = rank, `Pos_1` = value. |
| `SpellTotems`, `SpellReagents` | 1 079 / 3 338 | Shaman totem requirements, reagents. |
| `SpellShapeshiftForm` | 32 | Form definitions: 17 Battle Stance, 18 Defensive, 19 Berserker, 1 Cat, 5 Bear, 8 Dire Bear, 16 Ghost Wolf … (`CombatRoundTime`, `DamageVariance` for forms with own attacks). |
| `SpellLabel` | 4 721 | Tag ids per spell (label 25 = on every Warrior ability); useful for "affects all spells with label X" effects. |
| `SpellItemEnchantment` | 2 216 | Enchants incl. runes: `Effect_i` (1 proc, 2 damage, 3 equip-spell, 4 resistance, 5 stat), `EffectArg_i` (spell/stat id), `EffectPointsMin_i`. |
| `SpellDescriptionVariables` | 93 | `$name=${…}` macros referenced from descriptions. |
| `PowerType`, `ChrClassesXPowerTypes` | 6 / 15 | Power enums (0 mana, 1 rage, 2 focus, 3 energy, 4 combo points) and which class has which. Rage `DisplayModifier` 10 → stored ×10. Forever gives Warriors combo points too (row 189). |
| `ChrRaceRacialAbility` | 40 | Character-creation text only (no spell ids) — use the racial skill lines instead. |
| `ChrSpecialization` | 10 | One row per class, unused by Classic-style talents. |
| `PlayerExpectedStat` | 1 107 | Per level × class: `BaseMana`, `CritPerAgility`, `SpellCritPerIntellect`. Character model input, not spell data. |
| Ignore | | `CooldownSet*` (UI cooldown manager), `TraitCurrencySource`, `TraitCost`, `TraitTreeXTraitCost`. |

## 1.3 Class → spellbook

```
class_bit  = 1 << (ChrClasses.ID - 1)      Warrior 1, Paladin 2, Hunter 4, Rogue 8, Priest 16, Shaman 64, Mage 128, Warlock 256, Druid 1024
lines      = SkillRaceClassInfo rows where ClassMask & class_bit and SkillLine.CategoryID == 7
spells     = SkillLineAbility rows where SkillLine in lines and Spell exists in SpellName
```

| Class (`ChrClasses.ID`) | `SpellClassSet` | Skill lines (`SkillLine.ID`) | `TraitTree` |
|---|---|---|---|
| Warrior 1 | 4 | 26 Arms, 256 Fury, 257 Protection | 1117 |
| Paladin 2 | 10 | 594 Holy, 267 Protection, 184 Retribution | 1100 |
| Hunter 3 | 9 | 50 Beast Mastery, 163 Marksmanship, 51 Survival (+261 Beast Training) | 1091 |
| Rogue 4 | 8 | 253 Assassination, 38 Combat, 39 Subtlety (+40 Poisons, cat. 9) | 1111 |
| Priest 5 | 6 | 613 Discipline, 56 Holy, 78 Shadow Magic (+2885 Meditation) | 1114 |
| Shaman 7 | 11 | 375 Elemental Combat, 373 Enhancement, 374 Restoration | 1082 |
| Mage 8 | 3 | 237 Arcane, 8 Fire, 6 Frost | 1112 |
| Warlock 9 | 5 | 355 Affliction, 354 Demonology, 593 Destruction (+2898 Explorer Imp, pet lines 188–207, 2887) | 1116 |
| Druid 11 | 7 | 574 Balance, 134 Feral Combat, 573 Restoration | 1089 |

`SkillLineAbility` columns that matter:

- `ClassMask` = `class_bit` → a **trainable/baseline ability**. `ClassMask` = 0 → the spell is
  granted by a talent or rune (still listed under the tab's skill line). Both kinds are class
  spells; the second kind is reached authoritatively through 1.4/1.5.
- `SupercedesSpell` = previous rank → walk it to build rank chains; `Spell.NameSubtext_lang`
  gives "Rank N". Ranks are separate spell IDs with their own effects (Battle Shout r1 6673 → r7
  25289).
- `AcquireMethod` (retail semantics, inferred here): 0 trainer, 1 auto-learned at level, 2 granted (racials, generic), 3 = Forever
  baseline additions (Raging Blow 402911, Devastate 403196, Commanding Shout 403215, Enraged
  Regeneration 402913 — SoD rune abilities made baseline).
- `MinSkillLineRank`, `Trivial*`, `NumSkillUps`, `UniqueBit`, `TradeSkillCategoryID`: profession
  bookkeeping, ignore.

Alternative, broader net: `SpellClassOptions.SpellClassSet == ChrClasses.SpellClassSet` returns
every spell of the class *family* (Warrior: 481), including hidden trigger/proc spells and stance
passives that are not in any skill line. Use it to find the pieces a visible spell triggers, not
as the spellbook.

Stances/forms: the visible stance spell (Berserker Stance 2458) has `EffectAura` 36
`MOD_SHAPESHIFT` with `EffectMiscValue_0` = `SpellShapeshiftForm.ID` (19); its numbers live in a
hidden passive (7381 "Berserker Stance Passive", `ClassMask` 1 in the same skill line).

## 1.4 Class → talents (Trait system)

```
tree   = SkillLineXTraitTree[SkillLineID = first class line].TraitTreeID      (Warrior 26 → 1117)
nodes  = TraitNode where TraitTreeID == tree                                  (53 for Warrior)
entry  = TraitNodeXTraitNodeEntry[TraitNodeID] → TraitNodeEntry               (one entry per node here; MaxRanks 1..5)
def    = TraitDefinition[entry.TraitDefinitionID] → SpellID                   (the talent's spell; name via SpellName)
tab    = by TraitNode.PosX band: ≈1000–2800 Arms, ≈5000–6800 Fury, ≈9000–10900 Protection
         (authoritative: TraitNodeGroupDisplayInfo → TraitNodeGroupID ↔ SkillLineID; nodes belong to the tab group via TraitNodeGroupXTraitNode)
tier   = (TraitNode.PosY - 2130) / 600                                        (0..6)
column = (PosX - tab origin) / 600
```

- **Tier gating**: `TraitNodeGroupXTraitCond` attaches a `TraitCond` (`CondType` 0,
  `SpentAmountRequired` = 5·tier, `TraitCurrencyID` 3820) to the group holding the tier's nodes;
  `TraitCond.TraitNodeGroupID` is the group whose points are counted (all lower tiers of the same
  tab). I.e. the vanilla rule: tier N needs 5·N points in that tab.
- **Prerequisites**: `TraitEdge` (`LeftTraitNodeID` → `RightTraitNodeID`, `Type` 2). Warrior:
  Improved Tactical Mastery → Anger Management, Improved Rend → Deep Wounds, Sweeping Strikes →
  Mortal Strike, Enrage → Flurry, Death Wish → Bloodthirst, Concussion Blow → Shield Slam, Shield
  Specialization → Master of Defense, Improved Bloodrage → Last Stand.
- **Points**: `TraitCurrency` 3820, `SourcedMax` 51 (one per level from 10 via
  `TraitCurrencySource`). Trees 1187–1189 (currency 4225 "Legacy Point") are a separate
  profession-perk system (Master Chef, Bartering …), not class talents.
- **Ranks**: a multi-rank talent is **one spell** (`MaxRanks` up to 5), not five spells. The
  per-rank value comes from `TraitDefinitionEffectPoints` (`TraitDefinitionID`, `EffectIndex`,
  `OperationType` 0 = *replace* base points, `CurveID`) → `CurvePoint` rows with that `CurveID`:
  `Pos_0` = rank, `Pos_1` = value for `SpellEffect[EffectIndex].EffectBasePointsF`. The value
  stored in `SpellEffect` itself is arbitrary (Cruelty 5, Impale 10, Enrage 0) — always use the
  curve. Examples: Cruelty 1/2/3/4/5, Impale 10/20, Deep Wounds 20/40/60, Improved Heroic Strike
  −10/−20/−30 (= 1/2/3 rage), Flurry 5/10/15/20/25, Dual Wield Specialization has three curves
  (effects 0/1/2: 5..25 off-hand damage, 20..100 off-hand rage, 2..10 off-hand hit). Talents
  with `MaxRanks` 1 have no effect-points rows (their spell is used as-is).
- Forever-only Warrior talents (spell ≥ 1 200 000): Boundless Rage, Spearing Strike, Raging
  Blows, Vanguard, Bloodthrill, Weaponmaster (1290261, replaces the three weapon-spec talents),
  Precision, Master of Defense; Focused Rage 29787 and Iron Will 12962 were moved in.

## 1.5 Class → runes (Engraving)

SoD-style runes. Skill line 2851 "Engraving" (`ClassMask` 1503 = all classes); each
`SkillLineAbility` row with `ClassMask == class_bit` is an "Engrave <slot> - <Rune>" spell
(Warrior: 29). Resolve it as

```
engrave.SpellEffect: Effect 54 (enchant item; retail name ENCHANT_ITEM_TEMPORARY), EffectMiscValue_0 = SpellItemEnchantment.ID
SpellItemEnchantment: Effect_i == 3 (equip spell) → EffectArg_i = spell granted while the item is worn
```

Usually two args: a "learn/enable" spell and the passive/ability itself (Engrave Gloves -
Devastate 403475 → enchant 6800 → 403355 + 403195 Devastate). The slot is in the engrave spell's
name. Rune abilities then replace baseline buttons through `EffectAura` 332
(`OVERRIDE_ACTIONBAR_SPELLS`, 426 uses in this build). Skill line 2853 "Runes" only holds the
per-slot enabling passives ("Chest Rune Ability" …), not content.

## 1.6 Race → racials

Skill lines of `CategoryID` 9 named "<Race> Racial" / "Racial - <Race>" (101 Dwarf, 124 Tauren,
125 Orc, 126 Night Elf, 220 Undead, 733 Troll, 753 Gnome, 754 Human, 2980 Skyborne).
`SkillLineAbility.RaceMasks_0` = `1 << (ChrRaces.ID - 1)` (Human 1, Orc 2, Dwarf 4, Night Elf 8,
Undead 16, Tauren 32, Gnome 64, Troll 128). The Forever race Skyborne is `ChrRaces` 95/96 (two
variants, High Order Alliance and Windshaper Horde, `PlayableRaceBit` 32/33) and uses `RaceMasks_1`
bits 1/2 instead; the exported `race_mask` is the 64-bit `RaceMasks_0 | RaceMasks_1 << 32`.
Racials are ordinary spells (Blood Fury 20572, Berserking 20554, Sword Specialization 20597 …);
`ClassMask` −1 on a racial means "all classes".

## 1.7 Spell → what it does

### Per-spell header

- `SpellMisc.SchoolMask`: 1 physical, 2 holy, 4 fire, 8 nature, 16 frost, 32 shadow, 64 arcane.
- `SpellMisc.Attributes_0`: `0x40` passive, `0x80` hidden/do-not-display, `0x10` is-ability
  (uses weapon/melee rules). The other 16 attribute words are retail `SpellAttr1..16`; consult
  TrinityCore `SharedDefines.h` when a specific behaviour matters (e.g. `Attributes_1 & 0x4`
  channeled).
- `SpellMisc.Attributes_1 & 0x0800_0000` (`DISCOUNT_POWER_ON_MISS`): the cost is refunded when
  the attack is missed, dodged or parried. The tables do not give the amount; the engine refunds
  80 % (`POWER_REFUND_ON_MISS`). Set on the single-target warrior attacks, not on Whirlwind,
  Cleave, Thunder Clap or the shouts. Other failed casts pay the full cost.
- `SpellMisc.Attributes_2 & 0x0010_0000` (`BEHIND_TARGET`): the caster must be behind the target
  (Backstab, Garrote, Ambush); a tanking character gets `NotBehindTarget`.
- `SpellMisc.Attributes_3 & 0x400` (`MAIN_HAND`) / `& 0x0100_0000` (`REQUIRES_OFF_HAND_WEAPON`):
  a weapon `SpellEquippedItems` requirement must be met by that hand's weapon itself, as the
  server's `Spell::CheckItems` (Backstab needs the dagger in the main hand; Mutilate needs two).
  A spell with `REQUIRES_OFF_HAND_WEAPON` and weapon damage strikes with the off hand (off-hand
  roll, off-hand weapon damage × the off-hand penalty): Mutilate's off-hand strike.
- Finishers: a spell with a `COMBO_POINTS` power cost needs at least one point. Its effects,
  its aura's duration and its aura values read the points before they are spent; a finisher
  that fails keeps them. The count spent is on the cast report (statistics: "Finishers").
- Duration: `SpellDuration[DurationIndex].Duration` ms; −1 = until cancelled (stances).
  Finishers add `DurationPerResource` ms per combo point spent, capped at `MaxDuration`
  (Slice and Dice 6000 + 3000 × CP, max 21000); both are exported (`duration_per_resource_ms`,
  `max_duration_ms`) only when `DurationPerResource` is set, since elsewhere `MaxDuration` is a
  level-scaling cap the simulator does not use. A cast's aura then lasts that duration through
  the `DURATION` spell modifiers (Improved Slice and Dice +15/30/45 %).
- Cost: `SpellPower` rows (`OrderIndex` 0 primary). `PowerType` 1 (rage) values are ×10
  (Mortal Strike `ManaCost` 300 = 30 rage); mana/energy are as-is; `PowerCostPct` = % of base
  mana; `ManaPerSecond` for channels.
- Cooldown: `SpellCooldowns.RecoveryTime` ms; `CategoryRecoveryTime` = shared with the spell's
  category (Mortal Strike/Bloodthirst 6000, Whirlwind 10000); `StartRecoveryTime` = GCD it
  triggers (1500; stances 0 but 1000 on their category).
- Level: `SpellLevels.SpellLevel` is the level used for per-level scaling, `BaseLevel` the
  learn level, `MaxLevel` the scaling cap (0 = none).
- Cast time: `SpellCastTimes[CastingTimeIndex].Base` ms (Slam index 16 = 1500). Range:
  `SpellRange[RangeIndex]` (2 = melee). Radius: `SpellRadius[EffectRadiusIndex]` (Whirlwind 14 =
  8 yd, Battle Shout 9 = 20 yd).
- Category / hit table: `SpellCategories` — `Category` = shared-cooldown group (the
  `CategoryRecoveryTime` in `SpellCooldowns` applies to it), `StartRecoveryCategory` 133 = on
  the GCD, `DefenseType` 2 melee / 3 ranged / 1 magic / 0 none, `Mechanic`.
- Stance: `SpellShapeshift.ShapeshiftMask_0` (Overpower 65536 Battle, Whirlwind/Recklessness
  262144 Berserker, Rend 196608 Battle|Defensive, Execute 327680 Battle|Berserker; no row =
  any). Aura state: `SpellAuraRestrictions` (Execute `TargetAuraState` 2, Revenge
  `CasterAuraState` 1). Weapon: `SpellEquippedItems` (Shield Slam needs a shield). Targets:
  `SpellTargetRestrictions.MaxTargets`.

### `SpellEffect` rows

One row per effect, ordered by `EffectIndex` (0..n; descriptions refer to them as `$s1`, `$s2`…).

| column | meaning |
|---|---|
| `Effect` | what happens (SpellEffectName). `6` = apply aura, then `EffectAura` says which. |
| `EffectAura` | AuraType when `Effect` is 6 / 27 / 35 / 65 / 119 / 128 / 129 (area variants). |
| `EffectBasePointsF` | the value (exact float; modern format, **no** +1 die offset). Percent for percentage auras, flat otherwise. |
| `EffectRealPointsPerLevel` | value += this × (casterLevel − `SpellLevels.SpellLevel`), level capped at `MaxLevel`. |
| `Variance` | damage/heal range: value × (1 ± Variance/2). Frostbolt r11: 475 ± 3.7 %. |
| `EffectPointsPerResource` | value += this × combo points (finishers). An aura's value is taken when the cast applies it (Expose Armor −450 per point, Rupture's ticks); a new cast with other values re-applies the aura. |
| `EffectBonusCoefficient` | spell-power coefficient (Frostbolt r11 0.814, Lightning Bolt r10 0.714, Mind Blast r9 0.429 — the Classic values). 1 on melee rows is meaningless. |
| `BonusCoefficientFromAP` | attack-power coefficient (only Hammer of Wrath 429151 uses it; SoD-style AP scaling is otherwise expressed as a `DUMMY` + description). |
| `EffectAuraPeriod` | tick interval ms for periodic auras (Rend 3000). Ticks = duration / period; `$o1` = value × ticks. |
| `EffectAmplitude`, `EffectChainAmplitude`, `EffectChainTargets` | chain/multiplier data (Chain Lightning); Execute abuses `EffectChainAmplitude` 1.5. |
| `EffectTriggerSpell` | spell fired by `TRIGGER_SPELL` (64), `PROC_TRIGGER_SPELL` (aura 42), `PERIODIC_TRIGGER_SPELL` (aura 23). Follow it — the payload is usually a hidden spell (Deep Wounds 12834 → 12162, Unbridled Wrath 12322 → 12964). |
| `EffectMiscValue_0/1` | aura-specific: school mask (aura 13/14/22/79/87: 1 physical, 127 all), stat index (aura 29: −1 all, 0 Str, 1 Agi, 2 Sta, 3 Int, 4 Spi), form id (aura 36), power type (aura 24/85), **SpellModOp** (aura 107/108), mechanic (77), arbitrary for `DUMMY`. |
| `EffectSpellClassMask_0..3` | for auras 107/108/…: which spells are modified — matched (bitwise AND) against the target spell's `SpellClassOptions.SpellClassMask_0..3`, within the same `SpellClassSet`. |
| `ImplicitTarget_0/1` | 1 caster, 5 pet, 6 enemy target, 21 friendly target, 25 any target, 20 caster's party area, 15/16 area enemies, 22 caster position, 18/53/87 destinations. |
| `EffectRadiusIndex_0/1` | → `SpellRadius.Radius` yd. |
| `EffectMechanic` | mechanic applied by the effect — retail `Mechanics` enum: 11 snare, 12 stun, 15 bleed, 26 interrupt, 7 root, 9 silence, 3 disarm, 5 fear. |
| `EffectItemType` | item created (Create Item effects). |
| `Coefficient`, `ResourceCoefficient`, `ScalingClass`, `PvpMultiplier`, `GroupSizeBasePointsCoefficient`, `EffectAttributes` | unused/constant for player spells in this build. |

**`Effect` values seen on player spells** (count): 6 APPLY_AURA (5595), 2 SCHOOL_DAMAGE (885),
3 DUMMY (437, scripted), 10 HEAL (227), 30 ENERGIZE (122), 121 NORMALIZED_WEAPON_DMG (112:
weapon damage + basepoints, Mortal Strike/Whirlwind), 31 WEAPON_PERCENT_DAMAGE (108: basepoints
= %), 28 SUMMON, 35 APPLY_AREA_AURA_PARTY, 58 WEAPON_DAMAGE (weapon + basepoints, school of
spell), 24 CREATE_ITEM, 64 TRIGGER_SPELL (on an enemy, from a melee spell: a strike cast only
when the spell lands, which cannot miss again and is no ability of its own: Mutilate), 77 SCRIPT_EFFECT, 63 THREAT (flat threat, Sunder
Armor), 68 INTERRUPT_CAST, 38 DISPEL, 96 CHARGE, 17 WEAPON_DAMAGE_NOSCHOOL (weapon +
basepoints, Heroic Strike), 114 ATTACK_ME (Taunt; 91 is THREAT_ALL), 65 APPLY_AREA_AURA_RAID, 27 PERSISTENT_AREA_AURA,
16 QUEST_COMPLETE, 54 ENCHANT_ITEM_TEMPORARY (runes), 36 LEARN_SPELL, 9 HEALTH_LEECH,
8 POWER_DRAIN, 33 OPEN_LOCK, 98 KNOCK_BACK, 108 DISPEL_MECHANIC. Others follow the retail
`SpellEffectName` enum.

**`EffectAura` values seen** (count): 4 DUMMY (795 — scripted, needs hand-written logic),
108 ADD_PCT_MODIFIER (693), 107 ADD_FLAT_MODIFIER (630), 332 OVERRIDE_ACTIONBAR_SPELLS (426,
runes), 3 PERIODIC_DAMAGE (342), 42 PROC_TRIGGER_SPELL (309), 22 MOD_RESISTANCE (212; misc 1 =
armor), 33 MOD_DECREASE_SPEED, 8 PERIODIC_HEAL, 99 MOD_ATTACK_POWER (103), 69 SCHOOL_ABSORB,
23 PERIODIC_TRIGGER_SPELL, 79 MOD_DAMAGE_PERCENT_DONE (misc = school mask), 29 MOD_STAT,
12 MOD_STUN, 77 MECHANIC_IMMUNITY, 226 PERIODIC_DUMMY, 87 MOD_DAMAGE_PERCENT_TAKEN,
319 MOD_MELEE_HASTE_3 (melee attack speed %, Flurry buff 30), 31 MOD_INCREASE_SPEED, 118 MOD_HEALING_PCT
(Mortal Strike −50), 10 MOD_THREAT (misc school mask, Berserker −20), 11 MOD_TAUNT,
13 MOD_DAMAGE_DONE (flat, misc school), 26 MOD_ROOT, 65 MOD_CASTING_SPEED_NOT_STACK,
109 ADD_TARGET_TRIGGER, 54 MOD_HIT_CHANCE, 39 SCHOOL_IMMUNITY, 124 MOD_RANGED_ATTACK_POWER,
15 DAMAGE_SHIELD, 112 OVERRIDE_CLASS_SCRIPTS (misc = script id, hand-written), 14
MOD_DAMAGE_TAKEN, 7 MOD_FEAR, 36 MOD_SHAPESHIFT, 24 PERIODIC_ENERGIZE, 52
MOD_WEAPON_CRIT_PERCENT (Cruelty), 55 MOD_SPELL_HIT_CHANCE, 53 PERIODIC_LEECH, 57
MOD_SPELL_CRIT_CHANCE, 5 MOD_CONFUSE, 49 MOD_DODGE_PERCENT, 290 MOD_CRIT_PCT (Berserker Stance
+3), 41 DISPEL_IMMUNITY, 16 MOD_STEALTH, 134 MOD_MANA_REGEN_INTERRUPT, 85 MOD_POWER_REGEN (Anger
Management: misc 1 rage), 280 MOD_ARMOR_PENETRATION_PCT (Weaponmaster 12284: percent of the
target's armor ignored by attacks with the weapon types the spell requires), 122 MOD_OFFHAND_DAMAGE_PCT, 166 MOD_ATTACK_POWER_PCT, 137
MOD_TOTAL_STAT_PERCENTAGE, 135 MOD_HEALING_DONE, 149 REDUCE_PUSHBACK, 271
MOD_SPELL_DAMAGE_FROM_CASTER (Hemorrhage: the target takes `base_points` % more damage from the
caster's spells in the effect's class mask, Rupture; a multiplier of the caster's own spells
only). Others: retail
`AuraType` enum (`SharedDefines.h`).

**`SpellModOp` (misc value of auras 107/108)** — verified: 14 POWER_COST (Improved Heroic
Strike −10 = −1 rage), 15 CRIT_DAMAGE (Impale +10 %). Retail enum for the rest: 0 damage/healing,
1 duration, 2 threat, 3 effect-1 value, 4 proc charges, 5 range, 6 radius, 7 crit chance,
8 all effect values, 9 pushback, 10 cast time, 11 cooldown, 12 effect-2 value, 13 target
resistance, 16 hit chance, 17 chain targets, 18 proc chance, 19 period, 21 GCD, 23 effect-3
value, 24 bonus coefficient, 26 PPM, 27 amplitude.

### Talent/set-bonus modifiers resolve like this

```
modifier spell (talent) effect: EffectAura 107|108, EffectMiscValue_0 = SpellModOp, EffectBasePointsF = amount,
                                EffectSpellClassMask_0..3 = family flags
applies to spell X  iff  SpellClassOptions[X].SpellClassSet == SpellClassOptions[talent].SpellClassSet
                         and (SpellClassOptions[X].SpellClassMask_i & EffectSpellClassMask_i) != 0 for some i
```

Heroic Strike (all ranks) has `SpellClassMask_0` 64; Improved Heroic Strike's effect mask is 64.
Impale's mask is wide (all damaging abilities). Masks are signed 32-bit in the CSV (−2147483648
= bit 31). Talents without a class mask (Cruelty, 2H spec, Death Wish) are plain auras on the
character.

### Procs

`SpellAuraOptions` on the *aura* spell: `ProcChance` % (101 = "n/a, always"), `ProcCharges`
(Flurry buff 3), `ProcCategoryRecovery` internal cooldown ms, `SpellProcsPerMinuteID` →
`BaseProcRate`, `CumulativeAura` max stacks (Sunder Armor 5). `ProcTypeMask_0` = retail
`ProcFlags`: 0x4 done melee auto-attack, 0x10 done melee ability, 0x40/0x100 ranged auto/ability,
0x400/0x1000 done helpful/harmful non-damage spell, 0x4000/0x10000 done helpful/harmful magic
spell, 0x8/0x20/0x80/0x200/0x2000/0x20000 the "taken" counterparts, 0x40000 done periodic,
0x100000 taken any damage, 0x400000 done main-hand, 0x800000 done off-hand. E.g. Deep Wounds
69972 = any damaging attack/spell done; Enrage 139944 = any attack taken; Dual Wield Spec
8388612 = off-hand auto attacks. What the proc *does* is the trigger-spell effect (aura 42 →
`EffectTriggerSpell`) or the aura's own dummy.

### Descriptions

`Spell.Description_lang` tokens: `$s1`/`$m1`/`$M1` effect-1 value (min/max), `$o1` total over
duration, `$d` duration, `$t1` period, `$h` proc chance, `$n` proc charges, `$x1` chain targets,
`$a1` radius, `$i` max targets, `$u` stacks, `$<id>s1` / `$<id>d` another spell's value/duration,
`${expr}` arithmetic, `$/10;s1` divide, `$@spellicon<id>` icon, `$<name>` from
`SpellDescriptionVariables`. Good for sanity-checking an interpretation, not for values.

## 1.8 Worked examples (Warrior)

- **Mortal Strike 12294** (`NameSubtext` "Rank 1"): rage 30 (`ManaCost` 300), category 971 cd 6000,
  GCD 1500, duration 10 s. E0: aura 118 MOD_HEALING_PCT −50 on enemy target. E1: effect 121
  NORMALIZED_WEAPON_DMG +85. `SpellClassMask` [33554432, 0, 0, 0]; the spells sharing category 971
  have their own masks: Bloodthirst 23881 [33554432, 1024, 0, 0], Shield Slam 23922 [33554432, 1, 0, 0].
  Talent modifiers that name Mortal Strike in their masks therefore also hit Bloodthirst and Shield
  Slam through word 0 (Impale, Focused Rage).
  Skill line 26 with `ClassMask` 1 although the ability is a `TraitDefinition` spell, so `ClassMask` 0
  marks only some talent-granted spells (Deep Wounds, Improved Heroic Strike, …): decide what is a
  talent from the Trait tables, not from `ClassMask`. Ranks 2–4 are 21551–21553 via `SupercedesSpell`.
- **Rend r7 11574**: E0 aura 3 PERIODIC_DAMAGE 21 every 3000 ms, duration 21 s → 147 total; the server adds 2 % of attack power per tick (an
  `ATTACK_POWER_PER_TICK` override, the tables carry no coefficient).
- **Sunder Armor r5 11597**: E0 aura 22 MOD_RESISTANCE −450 misc 1 (armor), `CumulativeAura` 5,
  30 s; E1 effect 63 THREAT 206 (1013 before build 1.60.1.70009).
- **Flurry 12319** (talent, 5 ranks): aura 4 DUMMY, `ProcTypeMask` 0x15554 (crits), triggers the
  buff **12966**: aura 319 haste 30, 15 s, `ProcCharges` 3 consumed on `ProcTypeMask` 0x4 (melee
  auto-attacks). Rank scaling of the 30 needs the curve.
- **Deep Wounds 12834**: aura 42 PROC_TRIGGER_SPELL → 12162 (the bleed), description says
  "$m1 % of weapon damage over $412609d" — the value is rank-curve driven.
- **Improved Heroic Strike 12282**: aura 107 op 14 (cost) −10 mask 64 → Heroic Strike costs 1
  rage less per rank.
- **Berserker Stance 2458 → 7381**: aura 290 crit +3, aura 87 damage taken +10 (all schools),
  aura 10 threat −20, aura 166 AP % (0).

## 1.9 What is missing from the dump

Nothing required. Not dumped and not needed: `SpellProcsPerMinuteMod` (haste/spec PPM
modifiers; the 11 `SpellProcsPerMinute` rows are plain rates), `SpellScaling` (retail level
scaling; this build scales through `EffectRealPointsPerLevel` instead), `SpellXSpellVisual`,
`SpellKeyboundOverride`. Overpower has no `SpellAuraRestrictions` row in this build — its
"after the target dodges" condition must be implemented elsewhere (proc/override), check in
game.

Not in any class walk: the buffs *other* players and consumables provide (Blessings, Gift of
the Wild, totems, elixirs, food, the target debuffs of other classes). Their aura spells are in
the dump like any other; `data/external_buffs.yaml` lists the ones the simulator offers and
`export-spells --externals` writes them to `data/spells/externals.yaml` (see `README.md`).

## 1.10 What the exporter leaves out (pruning)

A damage simulator has no use for most of what the tables describe: crowd control, movement,
immunities, healing, phasing, mounts. `csim-tables export-spells` therefore prunes the walk
before writing `data/spells/*.yaml` (`crates/csim-tables/src/export/prune.rs`):

1. **Effects** whose kind or aura is on the discard list are removed from their spell; the
   remaining effects keep their table `EffectIndex` (so `$s2`, `POINTS_INDEX_1` and override
   `index` values still mean what the tables say). The list is
   `data/spells/overrides/discard.txt`; the engine's vocabulary (`crates/csim-engine/src/spell/dbc/
   aura.rs`, `effect.rs`) does not name these values — they load as `UNKNOWN_<id>` — and
   `dbc/discard.rs` lists their ids as `DISCARDED_AURA_IDS` / `DISCARDED_EFFECT_IDS` so the
   exporter can tell them from a genuinely new value. Kept although the list names them as
   candidates: `MOD_THREAT` / `MOD_TOTAL_THREAT` (stance passives, Defiance — threat is
   simulated), `OVERRIDE_ACTIONBAR_SPELLS` (Improved Slam, runes) and `ADD_TARGET_TRIGGER`
   (Relentless Strikes' energy on finishers; scripted, since its chance rule is server-side).
2. **Spells** left with no effects are dropped (Taunt: `ATTACK_ME` + `MOD_TAUNT`), then every
   `TRIGGER_SPELL` / `PROC_TRIGGER_SPELL` / action-bar override that pointed at a dropped spell,
   which can empty further spells (Intimidating Shout: fear, run speed and the stun it
   triggers; Improved Hamstring: a proc whose only payload is a root) — repeated until nothing
   changes. `SupercedesSpell` links to dropped ranks are cleared. Mocking Blow keeps its
   `SCHOOL_DAMAGE` and stays; Bloodthirst loses its run-speed aura and stays.
3. Spells the overrides mention (their own entry, or another entry's `params.spell` /
   `stance_passive`) are never dropped, even when empty: Berserker Rage keeps existing as the
   spell Improved Berserker Rage's `GAIN_RESOURCE_ON_USE` reacts to.

The exporter prints what it pruned. Build 1.60.1.70009: 63 effects and 16 spells from the
Warrior walk, 32 effects and 12 spells from the racials. A talent whose spell was pruned
(Iron Will, Improved Hamstring) has nothing to do in the simulator; the talent data will list
it without a spell.

## 1.11 Hand-written overrides (`data/spells/overrides/*.yaml`)

The tables carry the numbers; the overrides carry what retail keeps server-side or what only a
simulator cares about. Schema: `crates/csim-engine/src/spell/overrides.rs` (`OverrideFile`,
`SpellOverride`); the engine refuses unknown fields, references to spells that are not in the
data, and scripts missing their parameters.

```yaml
defaults:
  proc_hit_mask: [NORMAL, CRITICAL]  # hit results a proc fires on unless its entry says otherwise
overrides:
  - id: 12834                        # SpellName.ID
    note: why the entry exists
    proc: { hit_mask: [CRITICAL] }   # ProcHitMask: NORMAL CRITICAL MISS FULL_RESIST PARTIAL_RESIST
                                     #   DODGE PARRY BLOCK EVADE IMMUNE DEFLECT ABSORB REFLECT
                                     #   INTERRUPT FULL_BLOCK
                                     # chance_effect: N — the proc chance is aura effect N's value
                                     #   (talents whose rank value is the chance: Unbridled Wrath)
                                     # hand: MAINHAND — only that hand's attacks trigger it
                                     # target_aura: 772 — only while the character's aura of
                                     #   that spell (any rank) is up (Bloodthrill: your Rend)
                                     # finisher: true — fires when a finisher spends its combo
                                     #   points (source FINISHER), not on its ProcTypeMask
                                     #   (Ruthlessness, Improved Expose Armor)
                                     # family_mask: [4, 0, 0, 0] — only events of the spells
                                     #   of this class mask (spell_proc.SpellFamilyMask:
                                     #   Puncturing Wounds on Backstab)
                                     # family_mask_effect: 0 — as family_mask, aura effect
                                     #   0's own SpellClassMask (Head Rush, Revealed Flaw)
                                     # combo_points_effect: 2 — the finisher spent at least
                                     #   aura effect 2's value (Improved Expose Armor: 5)
                                     # chance_per_combo_point: true — the chance times the
                                     #   combo points spent (Relentless Strikes: 20 % each;
                                     #   a chance_effect without a value gives its points
                                     #   per resource: Revealed Flaw 5 %)
                                     # builder: true — only events of a spell that awards
                                     #   combo points (Seal Fate)
    effects:                         # scripts for DUMMY effects / auras, by EffectIndex
      - { index: 0, script: DEEP_WOUNDS_BLEED, params: { duration_spell: 412609 } }
    threat: { flat: 145, modifier: 1.0 }
    sim_flags: [RESETS_SWING_TIMERS]
    stance_passive: 7381             # the hidden passive that carries a stance's numbers
    on_event: [{ source: MELEE_DODGE, script: ADD_COMBO_POINTS, params: { value: 1 } }]
    debuff_priority: high            # slot priority of the spell's debuff (low / mid / high)
    debuff_shared: true              # one raid-wide instance (default: true when it stacks)
    ends_auras: [29604]              # these spells' buffs end with this one (Jom Gabbar's stacks)
    cast_time_ms: 1000               # in place of SpellCastTimes.Base (Charge's run to the target)
```

**Scripts** (`ScriptKind`; the interpreter implements each once, the data says where it applies):

| script | reads | params | used by |
|---|---|---|---|
| `ATTACK_POWER_PERCENT_DAMAGE` | `base_points` % of attack power as damage | — | Bloodthirst E1, Victory Rush, Shockwave |
| `EXECUTE` | `base_points` + `chain_amplitude` × 10 per rage above the cost; consumes all rage | — | Execute |
| `DEEP_WOUNDS_BLEED` | the trigger value (talent rank) % of the average base main-hand weapon damage (no attack power, off-hand crits too) over the aura's duration | `duration_spell` | Deep Wounds payload 12162 |
| `TRIGGER_WITH_VALUE` | casts `spell` with effect `effect` set to this aura's value | `spell`, `effect` | Flurry 12319 → 12966, Enrage |
| `TRIGGER_SPELL` | the proc casts `spell`, the server's payload: of a `DUMMY` proc aura, of a `PROC_TRIGGER_SPELL` without a trigger spell, or in place of the table's trigger; on a direct (non-aura) effect the cast casts `spell` | `spell` | Windfury Totem's party aura → 10610, Touch of the Grave 1260189 → 1260198, Relentless Strikes 14179 → 1314102, Seal Fate 14186 → 14189, Cutthroat 462708 → 462707, Vanish's `SANCTUARY` → Stealth 1787 |
| `PERIODIC_RESOURCE_GAIN` | `base_points` of `resource` every `period_ms` | `period_ms`, `resource` | Anger Management |
| `STANCE_RAGE_RETAINED` | rage kept on stance change += `base_points` | — | Tactical Mastery |
| `OFFHAND_RAGE_PERCENT` | off-hand rage generation += `base_points` × `value` (1 if absent) % | — (`value` optional) | Dual Wield Specialization E1 (10-50 %) |
| `GAIN_RESOURCE_ON_USE` | gain `base_points` (stored units) of `resource` when `spell` is used | `spell`, `resource` | Improved Berserker Rage |
| `EXTRA_ATTACK` | extra attacks from `spell` | `spell` | weapon specializations |
| `ENABLE_PROC` | while the aura is up the character has the hidden proc aura `spell` the server applies (its `ProcTypeMask`, weapon requirement, internal cooldown and payload come from its record), firing with this effect's value as its chance in percent; a weapon requirement is checked against the hand of the triggering attack | `spell` | Weaponmaster E2 → 12281 (sword extra attack) |
| `ENABLE_AURA` | while the aura is up the character has the hidden aura `spell` the server applies (gated by its own weapon requirement), with its effect `effect` set to this effect's value (a talent's rank value follows rank changes) | `spell`, `effect` | Weaponmaster E0 → 12700 (axe/polearm crit), E1 → 12284 (mace/staff armor penetration) |
| `ADD_COMBO_POINTS` | grants `value` combo points (Overpower's dodge marker); at most the class's `max_combo_points`, lapsing `combo_point_duration` s after the last gain (Warrior: 1 point, 6 s, so another dodge or Bloodthrill proc only refreshes it) | `value` | Overpower `on_event` |
| `RESET_COOLDOWN` | when the spell is cast, finishes the cooldowns of `spell`, or of every spell of its family in `family_mask` (as an event reaction: no runtime yet) | `spell` or `family_mask` | Preparation 14185 (every Rogue spell) |
| `WEAPON_TYPE_CRIT_PERCENT` | `base_points` % crit for attacks with the weapon types the spell's `SpellEquippedItems` accepts (all of them without one), per hand | — | Weaponmaster's hidden crit aura 12700 |
| `OFFHAND_HIT_CHANCE` | on a `MOD_HIT_CHANCE` aura effect: the hit chance counts for off-hand attacks only (the off-hand auto attack and off-hand strikes), not for the main hand's | — | Furious Precision 1323963 E0 |
| `WEAPON_TYPE_DAMAGE_PERCENT` | `base_points` % damage with the aura's required weapon types (no runtime yet) | — | — |
| `OFFHAND_COPY` | ability `spell` also strikes with the off-hand weapon: own roll, off-hand weapon damage × off-hand penalty, own `OFFHAND_SPELL` proc event, statistics as "<name> Off-Hand" | `spell` | none since the 70205 hotfixes (was Raging Blows on Whirlwind; see `OFFHAND_STRIKE`) |
| `TWO_HAND_ENERGIZE_MULTIPLIER` | an `ENERGIZE` effect gives `value` × its amount while a two-hand weapon is equipped | `value` | none since the 70205 hotfixes (was Unbridled Wrath's payload 12964) |
| `COMBO_POINT_AP_DAMAGE` | a finisher's attack power share: `value` % of attack power per combo point, or the `per_combo_point` entry (1 to 5 points). Without `effect` the effect deals it with the direct damage; with `effect` it is spread over that periodic aura's ticks, taken at the cast | `value` or `per_combo_point`, `effect` | Eviscerate E1 (3 %/point), Rupture E2 → E0 (4/10/18/21/24 %) |
| `ATTACK_POWER_PER_TICK` | `value` % of attack power added to every tick of this periodic aura effect, taken at the cast | `value` | Garrote E0 (3 %), Rend E0 (2 %) |
| `AP_COEFFICIENT` | the attack power coefficient the tables leave at 0 (`BonusCoefficientFromAP`): `value` × attack power added to a direct damage effect's hit, or to every tick (per stack) of a periodic damage aura, taken at the cast | `value` | Instant Poison VI E0 (0.005), Deadly Poison V E0 (0.0045), Thunder Clap E0 (0.03) |
| `WEAPON_TYPE_VALUE` | this effect's value replaces effect `effect`'s while the main-hand weapon's subclass is in `weapon_subclass_mask` | `effect`, `weapon_subclass_mask` | Ghostly Strike E3 → E0, Hemorrhage E4 → E3 (32768 = dagger) |
| `DAMAGE_PERCENT_VS_POISONED` | the spells this one triggers deal `base_points` % more while one of the caster's poisons (`DispelType` 4 debuff) is on the target | — | Mutilate E3 |
| `DAMAGE_PERCENT_BELOW_HEALTH` | the spells of `family_mask` deal `base_points` % more (a separate multiplier) while the target's health, from the encounter's progress, is below effect `effect`'s table value in percent | `effect`, `family_mask` | Quietus E0 (E1: 35 %) |
| `EXCLUSIVE_ARMOR_REDUCTION` | on a `MOD_RESISTANCE` debuff effect: the armor reduction shares one slot with the other exclusive ones, only the strongest applies (forever-bugs #112) | — | Sunder Armor E0, Expose Armor E0 |
| `NO_OP` | nothing; keeps the dummy (or an unknown aura) out of `csim-tables check` | — | markers, unmodelled halves, Bloodthrill payload 1282733 E1 aura 560 |

**Sim flags** (`SimFlag`): `IGNORED` (loaded, never cast, out of the rank groups),
`RESETS_SWING_TIMERS`, `STOPS_ATTACK_DURING_CAST`, `CANCELS_NEXT_SWING_QUEUE` (Slam),
`RUN_TO_TARGET` (Charge: the cast time is the run to the target, not a cast; spells that do not
hit the enemy and have no cast time of their own can be cast during it),
`START_OF_COMBAT` (passives whose ticking starts with combat), `CANNOT_CRIT`, `ENRAGE` (the buff
puts the character in the `ENRAGED` aura state that Raging Blow and Enraged Regeneration require;
the client tables do not carry the enrage mechanic), `OFFHAND_STRIKE` (a dual-wielding
character's ability also strikes with the off hand, as an `OFFHAND_COPY` aura makes it:
Whirlwind, whose off-hand strike is server side since the 70205 hotfixes).

**Event sources** (`on_event.source`, `ProcSource`): `MAINHAND_SWING`, `OFFHAND_SWING`,
`MAINHAND_SPELL`, `OFFHAND_SPELL`, `MELEE_HIT`, `MELEE_CRITICAL`, `MELEE_MISS`, `MELEE_DODGE`, `MELEE_PARRY`,
`MELEE_FULL_BLOCK`, `SPELL_HIT`, `SPELL_CRITICAL`, `SPELL_FULL_RESIST`, `RANGED_AUTO_SHOT`,
`RANGED_SPELL`, `ATTACK_TAKEN`, `MAGIC_SPELL`, `PERIODIC_DAMAGE` (a damaging tick,
`DEAL_HARMFUL_PERIODIC`), `FINISHER` (a finisher spent its combo points).

**Proc events and the spell behind them.** A cast's events carry the spell that raised them
(a triggered strike's own class mask, the cast's combo points), which `family_mask`,
`combo_points_effect` and the charged spell modifier auras read: Cold Blood is used by
Mutilate's strikes, not by Mutilate. A passive becomes a proc only when it has a payload (a
direct effect, a proc trigger with a trigger spell, a `TRIGGER_SPELL` / `TRIGGER_WITH_VALUE`
script); `PROC_TRIGGER_SPELL_WITH_VALUE` casts its trigger spell with the aura's value as
the payload's first effect value. A passive's buff is its owner's even when its effect names
an enemy target (who the payload hits). A spell modifier aura with a `ProcTypeMask` but no
charges is used up whole, every stack, by the first spell it modifies (Thousand Cuts).

Not overridable on purpose: costs, cooldowns, damage, durations, class masks, ranks and proc
sources — they come from the tables. Spells the overrides mention are never pruned (§1.10).
