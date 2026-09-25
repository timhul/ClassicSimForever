# 1. Playable class → talents → talent effects

How to go from a class (Warrior) to its talent tree, and from each talent to the concrete thing
it does (a stat aura, a modifier on specific spells, a proc, a new ability, a spell replacement,
or a scripted "dummy"). Verified on build `1.60.1.70009` with the full Warrior tree; the other
classes use the same tables. Companion docs: `SPELL_INSTRUCTIONS.md` (spell tables, effect and
aura enums), `ITEM_INSTRUCTIONS.md`.

## 1.1 Caveats

- **Do not use `Talent` / `TalentTab`.** They are the vanilla tables and are stale in this build:
  they still describe 5-rank talents as five spell IDs, and 124 of those IDs no longer exist in
  any spell table. Forever's talents live in the retail *Trait* system described here.
- A multi-rank talent is **one spell**. The rank-dependent numbers are *not* in `SpellEffect`
  (the value stored there is arbitrary); they come from `CurvePoint` (1.5).
- Forever changed the trees: new talents (spell IDs ≥ 1 200 000: Boundless Rage, Spearing
  Strike, Raging Blows, Vanguard, Bloodthrill, Weaponmaster, Precision, Master of Defense),
  moved talents (Focused Rage, Iron Will), and re-tuned values (Enrage 2–10 %, Flurry 5–25 %,
  Dual Wield Specialization has three effects).
- Talent-spell `Description_lang` in `Spell` is the best sanity check for what a `DUMMY` effect
  means, but never a source of numbers.

## 1.2 The chain

```
ChrClasses.ID (Warrior 1)
  └─ SkillRaceClassInfo.ClassMask & (1 << ID-1)  →  class SkillLine (CategoryID 7; Warrior 26 Arms, 256 Fury, 257 Protection)
       └─ SkillLineXTraitTree.SkillLineID (one of them, Warrior 26)  →  TraitTreeID          (Warrior 1117)
            └─ TraitNode.TraitTreeID == tree                                                  (one node = one talent, 53 for Warrior)
                 ├─ TraitNodeGroupXTraitNode → TraitNodeGroup → TraitNodeGroupDisplayInfo.SkillLineID   (which tab)
                 ├─ TraitNodeGroupXTraitCond → TraitCond.SpentAmountRequired                            (tier gate)
                 ├─ TraitEdge.RightTraitNodeID == node → LeftTraitNodeID                                (prerequisite talent)
                 └─ TraitNodeXTraitNodeEntry → TraitNodeEntry.MaxRanks
                      └─ TraitNodeEntry.TraitDefinitionID → TraitDefinition.SpellID                     (THE talent spell)
                           ├─ SpellName / Spell (name, description)
                           ├─ SpellEffect rows (what it does)  ── EffectSpellClassMask → SpellClassOptions of the modified spells
                           │                                   ── EffectTriggerSpell   → the proc / replacement payload
                           ├─ SpellAuraOptions (proc chance / flags)
                           └─ TraitDefinitionEffectPoints.TraitDefinitionID → CurveID → CurvePoint (Pos_0 rank, Pos_1 value per EffectIndex)
```

| Table | Key columns used |
|---|---|
| `SkillLineXTraitTree` | `SkillLineID`, `TraitTreeID` (9 rows, one per class) |
| `TraitTree` | `ID` (1117 Warrior, 1100 Paladin, 1091 Hunter, 1111 Rogue, 1114 Priest, 1082 Shaman, 1112 Mage, 1116 Warlock, 1089 Druid) |
| `TraitNode` | `ID`, `TraitTreeID`, `PosX`, `PosY` (layout), `Type` 0 |
| `TraitNodeXTraitNodeEntry` | `TraitNodeID` → `TraitNodeEntryID` (1:1 in this build) |
| `TraitNodeEntry` | `ID`, `TraitDefinitionID`, `MaxRanks` (1–5) |
| `TraitDefinition` | `ID`, `SpellID`; `OverrideName/Description_lang` (empty here) |
| `TraitDefinitionEffectPoints` | `TraitDefinitionID`, `EffectIndex`, `OperationType` (0 = set), `CurveID` |
| `CurvePoint` | `CurveID`, `Pos_0` = rank, `Pos_1` = value |
| `TraitNodeGroup`, `TraitNodeGroupXTraitNode` | node ↔ group membership (one "tab" group per tab + one group per tier band) |
| `TraitNodeGroupDisplayInfo` | tab group → `SkillLineID` (26 Arms, 256 Fury, 257 Protection), `OrderIndex` |
| `TraitCond`, `TraitNodeGroupXTraitCond` | tier gating (1.4) |
| `TraitEdge` | `LeftTraitNodeID` → `RightTraitNodeID`, `Type` 2 = prerequisite |
| `TraitCurrency` 3820, `TraitCurrencySource` | talent points: `SourcedMax` 51, one per level from 10 |
| Ignore | `TraitCost*`, `TraitTreeXTraitCurrency`, `TraitSystem`, trees 1187–1189 (currency 4225 "Legacy Point": profession perks, not talents), `TraitSubTree*` (unused) |

## 1.3 Warrior: finding the tree and its nodes

```
SkillRaceClassInfo: ClassMask & 1  and SkillLine.CategoryID == 7  →  26, 256, 257
SkillLineXTraitTree: SkillLineID 26  →  TraitTreeID 1117
TraitNode where TraitTreeID == 1117  →  53 nodes
```

Layout (positions are in a 600-unit grid):

```
tab    = TraitNodeGroupDisplayInfo of the node's groups: 11650 → SkillLine 26 Arms, 11657 → 256 Fury, 11670 → 257 Protection
         (equivalent: PosX ≈ 1020–2820 Arms, 5020–6820 Fury, 9080–10880 Protection)
tier   = (PosY − 2130) / 600            → 0..6
column = (PosX − tab_origin) / 600       → 0..3
```

For each node: `TraitNodeXTraitNodeEntry` → `TraitNodeEntry` (`MaxRanks`) → `TraitDefinition.SpellID`
→ `SpellName`. Example: node 105941 (Arms, tier 6, PosX 1620) → entry → definition → spell 12294
"Mortal Strike", `MaxRanks` 1.

## 1.4 Tier gating and prerequisites

- **Tiers**: each tab has one group per tier band. `TraitNodeGroupXTraitCond` attaches a
  `TraitCond` to the group of tier-N nodes; that cond has `CondType` 0, `SpentAmountRequired`
  = 5·N, `TraitCurrencyID` 3820, and its `TraitNodeGroupID` points at the group that contains
  tiers 0..N−1 of the same tab (the group whose spent points are counted). Net effect is the
  vanilla rule: tier N needs 5·N points in that tab. Warrior Arms: tier 1 needs 5 spent in
  group 12825 (tier 0), tier 6 (Mortal Strike) needs 30 spent in group 11650 (all Arms).
- **Prerequisites**: `TraitEdge` rows with `RightTraitNodeID` = the node; `LeftTraitNodeID` must
  be maxed. Warrior: Improved Tactical Mastery → Anger Management, Improved Rend → Deep Wounds,
  Sweeping Strikes → Mortal Strike, Enrage → Flurry, Death Wish → Bloodthirst, Concussion Blow →
  Shield Slam, Shield Specialization → Master of Defense, Improved Bloodrage → Last Stand.
- **Points**: 51 (`TraitCurrency` 3820 `SourcedMax`); `TraitCurrencySource` grants 1 per level
  from level 10.

## 1.5 Rank values

```
for ep in TraitDefinitionEffectPoints where TraitDefinitionID == def:
    curve = CurvePoint rows where CurveID == ep.CurveID, ordered by Pos_0
    value_at_rank(r) = curve[Pos_0 == r].Pos_1          # replaces SpellEffect[EffectIndex == ep.EffectIndex].EffectBasePointsF
```

`OperationType` is always 0 (set). Effects without an effect-points row keep their
`EffectBasePointsF` at every rank. Talents with `MaxRanks` 1 usually have no rows (Last Stand
has a leftover 3-point curve — use rank 1). Examples: Cruelty 1/2/3/4/5, Impale 10/20, Improved
Heroic Strike −10/−20/−30 (rage ×10 → 1/2/3 rage), Deep Wounds 20/40/60, Dual Wield
Specialization effect 0: 5/10/15/20/25, effect 1: 20/40/60/80/100, effect 2: 2/4/6/8/10.

## 1.6 Talent spell → what it does

Read the talent spell's `SpellEffect` rows (`SPELL_INSTRUCTIONS.md` 1.7). Every Warrior talent
falls into one of these kinds, decided by `Effect` / `EffectAura`:

**A. Stat aura on the character** — `Effect` 6 with a plain stat/percent aura. The value is the
rank value; `EffectMiscValue_0` qualifies it (school mask, stat, skill, mechanic).
Cruelty 52 `MOD_WEAPON_CRIT_PERCENT` 1–5; Deflection 47 `MOD_PARRY_PERCENT`; Precision 54
`MOD_HIT_CHANCE` + 55 `MOD_SPELL_HIT_CHANCE`; Two-Handed Weapon Specialization / Bastion 79
`MOD_DAMAGE_PERCENT_DONE` misc 1 (physical). **Equipment/stance conditions of passives are in
`SpellEquippedItems` / `SpellShapeshift` on the talent spell** and the aura only applies while
they hold: Two-Handed Weapon Specialization 12163 → weapon class 2, subclass mask 136546
(2H axes/maces/swords, polearms, staves, spears); Bastion 16538 → armor class 4 mask 64
(shield); Defiance 12792 → shield **and** `ShapeshiftMask_0` 131072 (Defensive Stance); Dual
Wield Specialization 23584 → weapon mask 41105 (1H axes/maces/swords, fists, daggers).
Defiance 10 `MOD_THREAT` misc 127; Anticipation 30 `MOD_SKILL` misc 95 (Defense skill line);
Toughness 142 `MOD_BASE_RESISTANCE_PCT` misc 1 (armor from items); Boundless Rage 418 (max
rage, ×10); Iron Will 232 (mechanic duration, misc 1/12 = charm?/stun); Anger Management 85
`MOD_POWER_REGEN` misc 1 (rage); Death Wish 79 +20 physical / 87 `MOD_DAMAGE_PERCENT_TAKEN` +5 /
77 `MECHANIC_IMMUNITY` misc 5 (fear) — an *active* buff, its spell has cost/cooldown/duration.

**B. Modifier on specific spells** — `EffectAura` 107 `ADD_FLAT_MODIFIER` or 108
`ADD_PCT_MODIFIER`; `EffectMiscValue_0` = what is modified (SpellModOp); rank value = amount;
`EffectSpellClassMask_0..3` = which spells. Resolution:

```
affected = spells X with SpellClassOptions[X].SpellClassSet == SpellClassOptions[talent].SpellClassSet (Warrior 4)
           and any(SpellClassOptions[X].SpellClassMask_i & EffectSpellClassMask_i)
           restricted to X in the class's own spell list (skill lines 26/256/257 + their triggered spells)
```

The family also contains NPC/test spells with Warrior flags ("Breath of Sargeras", "Test
Strike"), hence the restriction. All ranks of a spell share the same class mask, so one talent
row covers every rank. Warrior ops seen: 14 `POWER_COST` (Improved Heroic Strike −1/−2/−3 rage →
Heroic Strike; Improved Cleave, Improved Execute −3/−5, Improved Sunder Armor, Improved Thunder
Clap −2/−4/−6, Focused Rage −1/−2/−3 → every offensive ability, Raging Blows −2 → Cleave),
0 `DAMAGE` (Improved Revenge +20/40/60 %), 22 `DOT` (Improved Rend +12/23/35 %), 15
`CRIT_DAMAGE` (Impale +10/20 % on all abilities), 7 `CRIT_CHANCE` (Improved Overpower
+25/50), 11 `COOLDOWN` ms (Improved Intercept −5/−10 s, Improved Disarm, Improved Shield Wall
−5.5/−11 min), 10 `CAST_TIME` + 21 `GCD` ms (Improved Slam −0.25/−0.5 s), 8 `ALL_EFFECTS`
(Improved Bloodrage +25/50 %, Improved Charge +3/+6 rage), 6 `RADIUS` (Booming Voice).

**C. Proc** — `EffectAura` 42 `PROC_TRIGGER_SPELL`: the rank value is the **proc chance %**
(overrides `SpellAuraOptions.ProcChance`), `EffectTriggerSpell` is the payload, and
`SpellAuraOptions.ProcTypeMask_0` says on what (0x4 melee auto-attack, 0x10 melee ability,
0x11154 any damaging hit, 0x222a8 any hit taken, 0x2a8 taken melee/ranged/spell hits — see
`SPELL_INSTRUCTIONS.md`). Read the payload's own `SpellEffect`. Unbridled Wrath 12/…/60 % →
12964 `ENERGIZE` 10 rage (×10 → 1 rage); Deep Wounds 20/40/60 % → 12162 (a `DUMMY` bleed:
"$m1 % of weapon damage over $412609d" — the *chance* is the rank value and the description's
`$m1` is the same number, so the bleed size is implemented in script); Shield Specialization
E1 20–100 % → 1310318 `ENERGIZE` 50 (5 rage) on block; Master of Defense 50/100 % → 23602
(5 rage on dodge/parry); Improved Hamstring 5/10/15 % → 23694 root; Improved Shield Bash
50/100 % → 18498 silence.

**D. Proc with a DUMMY payload** — `EffectAura` 4 on a spell whose `SpellAuraOptions` has a
`ProcTypeMask`: the mechanics are scripted; the rank value is the number in the description.
Flurry (mask 0x15554 = any crit): 5/10/15/20/25 % attack speed for `$12966n` = 3 swings —
the buff is 12966 (aura 319, 15 s, `ProcCharges` 3, consumed on mask 0x4 auto-attacks; its
stored 30 is overridden by the talent value). Enrage (mask 0x222a8, `ProcChance` 30): 2–10 %
physical damage for `$12880d`. Blood Craze: 1/2/3 % health over `$16488d`. Bloodthrill: 2–10 %
chance per melee attack on a Rend target to reset Overpower. Sweeping Strikes: `ProcCharges` 5.
Dual Wield Specialization E1 20–100 % (off-hand rage) with mask 0x800004 (off-hand auto).
Improved Berserker Rage 50/100 (rage ×10 → 5/10) and 50/100 % snare removal.

**E. Spell replacement** — `EffectAura` 332 `OVERRIDE_ACTIONBAR_SPELLS`: `EffectMiscValue_0` =
the spell replaced, `EffectBasePointsF` = the replacement. Improved Slam replaces each Slam rank
(1240193/1464/8820/11604/11605 → 1310196–1310200); Vanguard replaces Charge ranks 100/6178/11578
with Defensive-Stance-usable copies 1240287–89. Resolve the replacement spell like any other.

**F. Pure DUMMY** — `EffectAura` 4 with no proc mask: script only, rank value is the tooltip
number. Improved Tactical Mastery 3–15 rage kept on stance change; Weaponmaster three values
(per-weapon-type bonuses per the description); Raging Blows E0 (Whirlwind hits with off-hand; the `OFFHAND_COPY` override);
Anger Management E1–E3 (1 rage / 3 s, 30 % less decay).

**G. New ability** — `MaxRanks` 1 and the definition spell is a castable: Mortal Strike (aura
118 −50 % healing + effect 121 weapon + 85), Bloodthirst (`SCHOOL_DAMAGE` 30 + `DUMMY` 35 % AP),
Shield Slam (`SCHOOL_DAMAGE` 430 + dispel), Death Wish, Piercing Howl, Concussion Blow, Last
Stand, Sweeping Strikes, Spearing Strike (121 + 31 40 % weapon damage + dummy 2× vs
giants/dragonkin/…). The same spell also appears in `SkillLineAbility` with `ClassMask` 0
under the tab's skill line. Everything about it (cost, cooldown, stance, effects) resolves per
`SPELL_INSTRUCTIONS.md`.

## 1.7 Warrior talent reference (generated from the tables)

Ranks = `MaxRanks`; `←` = prerequisite (`TraitEdge`); values are `CurvePoint` per rank, or the
fixed `EffectBasePointsF`. Rage amounts are ×10. Modifier targets are restricted to Warrior
spells.

| Tab | Tier | Talent (spell) | Ranks | Kind | Effects (rank values) |
|---|---|---|---|---|---|
| Arms | 0 | Improved Heroic Strike (12282) | 3 | modifier | FLAT POWER_COST -10/-20/-30 → Heroic Strike |
| Arms | 0 | Deflection (16462) | 5 | stat aura | MOD_PARRY_PERCENT 1/2/3/4/5 |
| Arms | 0 | Improved Rend (12286) | 3 | modifier | PCT DOT 12/23/35 → Rend |
| Arms | 1 | Improved Charge (12285) | 2 | modifier | FLAT ALL_EFFECTS 30/60 → Charge |
| Arms | 1 | Improved Tactical Mastery (12295) | 5 | dummy | DUMMY 3/6/9/12/15 |
| Arms | 1 | Improved Overpower (12290) | 2 | modifier | FLAT CRIT_CHANCE 25/50 → Overpower |
| Arms | 2 | Anger Management (12296) ← Improved Tactical Mastery | 1 | stat aura, dummy | MOD_POWER_REGEN 15 misc 1; DUMMY 1; DUMMY 3; DUMMY 30 |
| Arms | 2 | Deep Wounds (12834) ← Improved Rend | 3 | proc | PROC 20/40/60% → 12162 (Deep Wounds) |
| Arms | 3 | Spearing Strike (1310222) | 1 | ability | NORMALIZED_WEAPON_DMG 0; WEAPON_PERCENT_DAMAGE 40; DUMMY 2 |
| Arms | 3 | Two-Handed Weapon Specialization (12163) | 3 | stat aura | MOD_DAMAGE_PERCENT_DONE 1/2/3 misc 1 |
| Arms | 3 | Impale (16493) | 2 | modifier | PCT CRIT_DAMAGE 10/20 → Bloodthirst, Cleave, Concussion Blow, Devastate … |
| Arms | 4 | Bloodthrill (1289682) | 5 | proc(dummy) | DUMMY 2/4/6/8/10 |
| Arms | 4 | Sweeping Strikes (12292) | 1 | proc(dummy) | DUMMY 0 |
| Arms | 4 | Weaponmaster (1290261) | 5 | dummy | DUMMY 1/2/3/4/5; DUMMY 3/6/9/12/15; DUMMY 1/2/3/4/5 |
| Arms | 5 | Improved Slam (12862) | 2 | modifier, override | FLAT CAST_TIME -250/-500 → Concussion Blow, Slam; FLAT GCD -250/-500 → Concussion Blow, Slam; replace 1240193→1310196; replace 1464→1310197; replace 8820→1310198; replace 11604→1310199; replace 11605→1310200 |
| Arms | 5 | Improved Hamstring (12289) | 3 | proc | PROC 5/10/15% → 23694 (Improved Hamstring) |
| Arms | 6 | Mortal Strike (12294) ← Sweeping Strikes | 1 | ability | MOD_HEALING_PCT -50 misc 127; NORMALIZED_WEAPON_DMG 85 |
| Fury | 0 | Booming Voice (12321) | 5 | modifier | PCT RADIUS 10/20/30/40/50 → Battle Shout, Demoralizing Shout |
| Fury | 0 | Cruelty (12320) | 5 | stat aura | MOD_WEAPON_CRIT_PERCENT 1/2/3/4/5 |
| Fury | 1 | Iron Will (12962) | 5 | stat aura | aura 232 -3/-6/-9/-12/-15 misc 1; aura 232 -3/-6/-9/-12/-15 misc 12 |
| Fury | 1 | Unbridled Wrath (12322) | 5 | proc | PROC 12/24/36/48/60% → 12964 (Unbridled Wrath) |
| Fury | 2 | Improved Cleave (12329) | 3 | modifier | FLAT POWER_COST -10/-20/-30 → Cleave |
| Fury | 2 | Piercing Howl (12323) | 1 | aura | MOD_DECREASE_SPEED -50 |
| Fury | 2 | Blood Craze (16487) | 3 | proc(dummy) | DUMMY 1/2/3; DUMMY 20 |
| Fury | 2 | Boundless Rage (1310236) | 3 | stat aura | aura 418 100/200/300 misc 1 |
| Fury | 3 | Dual Wield Specialization (23584) | 5 | proc(dummy), stat aura, aura | MOD_OFFHAND_DAMAGE_PCT 5/10/15/20/25; DUMMY 20/40/60/80/100; MOD_HIT_CHANCE 2/4/6/8/10 |
| Fury | 3 | Raging Blows (1310315) | 1 | modifier, dummy | DUMMY 100; FLAT POWER_COST -20 → Cleave |
| Fury | 3 | Enrage (12317) | 5 | proc(dummy) | DUMMY 2/4/6/8/10 |
| Fury | 3 | Improved Execute (20502) | 2 | modifier | FLAT POWER_COST -30/-50 → Execute |
| Fury | 4 | Precision (1225295) | 3 | stat aura | MOD_HIT_CHANCE 1/2/3; MOD_SPELL_HIT_CHANCE 1/2/3 |
| Fury | 4 | Death Wish (12328) | 1 | stat aura, aura | MOD_DAMAGE_PERCENT_DONE 20 misc 1; MECHANIC_IMMUNITY 100 misc 5; MOD_DAMAGE_PERCENT_TAKEN 5 misc 127 |
| Fury | 4 | Improved Intercept (20504) | 2 | modifier | FLAT COOLDOWN -5000/-10000 → Intercept |
| Fury | 5 | Improved Berserker Rage (20500) | 2 | proc(dummy) | DUMMY 50/100; DUMMY 50/100 |
| Fury | 5 | Flurry (12319) ← Enrage | 5 | proc(dummy) | DUMMY 5/10/15/20/25 |
| Fury | 6 | Bloodthirst (23881) ← Death Wish | 1 | ability | SCHOOL_DAMAGE 30; DUMMY 35; MOD_INCREASE_SPEED 10 |
| Protection | 0 | Shield Specialization (12298) | 5 | proc, stat aura | MOD_BLOCK_PERCENT 1/2/3/4/5; PROC 20/40/60/80/100% → 1310318 (Shield Specialization) |
| Protection | 0 | Anticipation (12297) | 5 | stat aura | aura 30 4/8/12/16/20 misc 95 |
| Protection | 1 | Improved Bloodrage (12301) | 2 | modifier | PCT ALL_EFFECTS 25/50 → Bloodrage |
| Protection | 1 | Toughness (12299) | 5 | stat aura | aura 142 2/4/6/8/10 misc 1; aura 466 2/4/6/8/10 |
| Protection | 1 | Improved Thunder Clap (12287) | 3 | modifier | FLAT POWER_COST -20/-40/-60 → Thunder Clap |
| Protection | 2 | Last Stand (12975) ← Improved Bloodrage | 1 | ability | DUMMY 5/10/15 |
| Protection | 2 | Master of Defense (1310316) ← Shield Specialization | 2 | proc | PROC 50/100% → 23602 (Master of Defense) |
| Protection | 2 | Improved Revenge (12797) | 3 | modifier | PCT DAMAGE/HEALING 20/40/60 → Revenge |
| Protection | 2 | Defiance (12792) | 3 | stat aura | MOD_THREAT 5/10/15 misc 127 |
| Protection | 3 | Improved Sunder Armor (12308) | 3 | modifier | FLAT POWER_COST -10/-20/-30 → Sunder Armor |
| Protection | 3 | Improved Disarm (12313) | 3 | modifier | FLAT COOLDOWN -7000/-13000/-20000 → Disarm |
| Protection | 3 | Vanguard (1310317) | 1 | override | replace 11578→1240289; replace 6178→1240288; replace 100→1240287 |
| Protection | 4 | Improved Shield Wall (12312) | 2 | modifier | FLAT COOLDOWN -330000/-660000 → Shield Wall |
| Protection | 4 | Concussion Blow (12809) | 1 | aura | MOD_STUN 0 |
| Protection | 4 | Improved Shield Bash (12311) | 2 | proc | PROC 50/100% → 18498 (Silenced) |
| Protection | 4 | Bastion (16538) | 5 | stat aura | MOD_DAMAGE_PERCENT_DONE 2/4/6/8/10 misc 1 |
| Protection | 5 | Focused Rage (29787) | 3 | modifier | FLAT POWER_COST -10/-20/-30 → Bloodthirst, Challenging Shout, Cleave, Concussion Blow … |
| Protection | 6 | Shield Slam (23922) ← Concussion Blow | 1 | ability | effect 38 1; SCHOOL_DAMAGE 430 |

## 1.8 The exported file

`csim-tables export-talents --class warrior` writes the walk above to `data/talents/warrior.yaml`
(schema: `crates/csim-engine/src/talent/spec.rs`): the build, class, `TraitTree.ID`, the points
(`TraitCurrency.SourcedMax`), the points per tier (the tier-1 gate of `TraitCond`), the tabs
(`TraitNodeGroupDisplayInfo` in `OrderIndex` order with the skill line's name) and one entry
per node:

```yaml
- node: 105950          # TraitNode.ID
  spell: 12834          # TraitDefinition.SpellID; its effects are in data/spells/warrior.yaml
  name: Deep Wounds     # SpellName, for readability
  tab: 26               # skill line of the tab
  tier: 2               # (PosY − top) / 600
  column: 2             # (PosX − tab's leftmost PosX) / 600
  max_ranks: 3          # TraitNodeEntry.MaxRanks
  requires: 105956      # TraitEdge left node (Improved Rend), absent when none
  rank_values:          # TraitDefinitionEffectPoints → CurvePoint, per EffectIndex, one per rank
    0: [20.0, 40.0, 60.0]
```

The tier rule (`points_per_tier × tier` points in the tab) and the prerequisite are the whole
gating model; the `TraitCond` rows are only checked against it at export time. The runtime
(`crates/csim-engine/src/talent/`) applies rank *r* by replacing the base points of effect
`index` of the talent spell with `rank_values[index][r − 1]` and enabling the spell: a passive's
aura goes up, a modifier lands in the spell modifier table, a proc arms with the value as its
payload's trigger value, an ability (and its trainable higher ranks) becomes castable. Effects
without a curve keep their table value.

## 1.9 Missing tables

None for talents. The retail names of a few aura ids used by Forever talents are unverified:
232 (mechanic duration modifier, Iron Will), 418 (max power, Boundless Rage), 466 (Toughness
second effect), 30 (`MOD_SKILL`, Anticipation). Their meaning is clear from the descriptions.
