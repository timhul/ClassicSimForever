# 1. Equippable items (`data/tables/Item*.csv`)

Everything below was verified against build `1.60.1.70009` by re-deriving the Classic values in
`data/items/*.yaml` (converted from ClassicSim) from the tables. Where a formula is given it
reproduced the Classic numbers exactly unless noted. The tables are the standard client DB2
tables (same layout as retail / Classic Era on wago.tools), so TrinityCore's `ItemTemplate.cpp`
is a good secondary reference for anything not covered here.

## 1.1 Caveats about this dump

- **`ItemSparse` is incomplete.** `Item` has 31 675 rows, `ItemSparse` only 19 172 in the client
  files (build 70009). Modern clients only ship a subset of `ItemSparse` and receive the rest as
  server-side hotfixes. `tools/fetch_hotfixes.py` applies them: 4 581 rows on build 70205
  (Fiery War Axe, Corpsemaker, the dungeon and PvP sets, ...). Some are still missing after that
  (Hand of Justice 11815). `Item`, `ItemEffect`, `ItemXItemEffect`, `ItemSet` look complete.
- **Three games' items.** Item IDs go up to ~286 000: Classic's below ~30 000, Season of
  Discovery's from 200 000 to 249 999 (Scarlet Enclave and its "Sanctified" items, SoD's remade
  PvP sets that share Classic's names, ...), Forever's own from 250 000 (every set with "1.60.0"
  bonuses is above 270 000, as are the "Premier" PvP sets). No `ItemSparse` or `Item` column tells
  SoD's apart (`ExpansionID`, `ContentTuningID`, the flags and `ItemNameDescriptionID` were
  compared on build 70205), so the export skips the id range. Classic items can also be re-tuned
  (e.g. Huge Thorium Battleaxe is 3.50 speed / +2 skill here vs 3.30 / +10 in Classic). Do not
  assume Classic values. Some Classic tier 0 sets (Battlegear of Valor, Beaststalker, ...) carry
  SoD's set bonuses ("S03 - Item - ..." spells); whether Forever uses them is unverified.
- Max `RequiredLevel` / `ItemLevel` on real items is 60 / ~92 (level-100 rows are `[PH]`/test
  items). No item uses `PlayerLevelToItemLevelCurveID`, `ItemLevelOffsetCurveID`, `ContentTuningID`,
  `ItemSquishEraID`, sockets or `SpellWeight` — ignore those columns.
- Skip rows with `ItemSparse.Flags_0 & 0x10` (deprecated: `OLD…`, `Deprecated …`, 1 601 rows)
  and names containing `TEST`, `[PH]`, `Monster - `, `QA `, `UNUSED`.

## 1.2 Table roles and join keys

```
Item.ID ──────────────┬── ItemSparse.ID            (1:0..1)  stats, level, quality, name
                      ├── ItemXItemEffect.ItemID ──> ItemEffect.ID ──> SpellName.ID   procs / on-use / on-equip spells
                      ├── ItemXBonusTree.ItemID ───> ItemBonusTree.ID <── ItemBonusTreeNode.ParentItemBonusTreeID
                      │                                 └─ .ChildItemBonusListID ──> ItemBonus.ParentItemBonusListID   (random suffixes)
                      └── ItemSet.ItemID_0..16  (ItemSparse.ItemSet == ItemSet.ID) ──> ItemSetSpell.ItemSetID ──> SpellName.ID
Item.ClassID/SubclassID ──> ItemSubClass.(ClassID,SubClassID)     ItemClass.ClassID
ItemSparse.ItemLevel ──> RandPropPoints.ID, ItemArmorTotal.ItemLevel, ItemArmorQuality.ID,
                         ItemArmorShield.ItemLevel, ItemDamage*.ItemLevel
ItemSparse.AllowableClass  bitmask over ChrClasses.ID  (bit = 1 << (ChrClasses.ID - 1))
ItemSparse.LimitCategory ──> ItemLimitCategory.ID     (unique-equipped groups)
ItemSparse.ItemNameDescriptionID ──> ItemNameDescription.ID  (grey subtitle text)
```

| Table | Rows | Use |
|---|---|---|
| `Item` | 31 675 | Physical identity: `ClassID`, `SubclassID`, `InventoryType`, `Material`, icon. Present for every item. `InventoryType`/`Material` are duplicated in `ItemSparse` (0 mismatches). |
| `ItemSparse` | 19 172 | Everything the sim needs: `Display_lang` (name), `ItemLevel`, `OverallQualityID`, `RequiredLevel`, `AllowableClass`, `ItemDelay` (ms), `DmgVariance`, `StatModifier_bonusStat_0..9` + `StatPercentEditor_0..9`, `QualityModifier` (bonus armor), `ItemSet`, `Bonding`, `MaxCount`, `LimitCategory`, `Flags_0..4`, `InventoryType`. |
| `ItemSubClass` | 100 | Weapon/armor sub-class names. Key is the pair (`ClassID`,`SubClassID`), **not** `ID`. |
| `ItemClass` | 17 | Class names. Key is `ClassID`, not `ID`. Only 2 (Weapon) and 4 (Armor) matter. |
| `RandPropPoints` | 100 | Stat budget per item level: `Epic_0..4`, `Superior_0..4`, `Good_0..4` (the `…F_` columns are identical floats). |
| `ItemArmorTotal`, `ItemArmorQuality`, `ItemArmorShield` | 100 each | Armor per item level. |
| `ItemDamageOneHand`, `ItemDamageTwoHand`, `ItemDamageRanged`, `ItemDamageWand`, `ItemDamageThrown`, `ItemDamageAmmo` | 100 each | Weapon DPS per item level. The `…Caster` tables are byte-identical to the non-caster ones in this build. |
| `ItemEffect`, `ItemXItemEffect` | 12.5k each | Item spells (proc / use / equip). |
| `ItemSet`, `ItemSetSpell` | 532 / 1 462 | Set membership and set bonuses. |
| `ItemBonus`, `ItemBonusList`, `ItemBonusTree`, `ItemBonusTreeNode`, `ItemXBonusTree` | | Random-suffix ("of the Bear") system. |
| `ItemLimitCategory` | 52 | Unique-equipped groups ("Signet Ring of the Bronze Dragonflight", quantity 1). |
| `ItemNameDescription` | 92 | Subtitle strings; also the suffix names used by `ItemBonus` type 5. |
| `SpellItemEnchantment` | | Enchants (Crusader = 1900, Fiery Weapon = 803). Not item data; the rogue poisons' rows (the `enchantment:` keys of `data/enchants.yaml`) go into `data/spells/enchants.yaml` with `export-spells --enchants`. |
| Ignore | | `ItemSearchName` (AH search subset), `ItemExtendedCost` (vendor prices), `ItemBonusListGroup`, `ItemCondition`, `ItemLimitCategoryCondition`, `ItemSpecOverride`, `ItemSubClassMask`, `PlayerCondition` (only 2 `ItemEffect` rows reference it). |

## 1.3 Enumerations (as observed in this build)

**`Item.ClassID`**: 2 = Weapon, 4 = Armor. Everything else is not equipment (0 consumable,
1 container, 7 trade goods, 9 recipe, 12 quest, 15 misc …).

**Weapon `SubclassID` (ClassID 2)**: 0 One-Handed Axes, 1 Two-Handed Axes, 2 Bows, 3 Guns,
4 One-Handed Maces, 5 Two-Handed Maces, 6 Polearms, 7 One-Handed Swords, 8 Two-Handed Swords,
10 Staves, 13 Fist Weapons, 14 Miscellaneous, 15 Daggers, 16 Thrown, 18 Crossbows, 19 Wands,
20 Fishing Pole. (9/11/12/17 exist in the table but are unused: Warglaives, Exotics, Spears.)

**Armor `SubclassID` (ClassID 4)**: 0 Miscellaneous (rings, necks, trinkets, cloaks, held
off-hands), 1 Cloth, 2 Leather, 3 Mail, 4 Plate, 6 Shield, 7 Libram, 8 Idol, 9 Totem.

**`InventoryType`** (retail enum; the ones seen on class 2/4 items): 1 Head, 2 Neck, 3 Shoulder,
4 Shirt, 5 Chest, 6 Waist, 7 Legs, 8 Feet, 9 Wrist, 10 Hands, 11 Finger, 12 Trinket,
13 One-Hand (either hand), 14 Shield, 15 Ranged (bow/gun/crossbow), 16 Back, 17 Two-Hand,
19 Tabard, 20 Robe (= chest), 21 Main Hand only, 22 Off Hand weapon, 23 Held in Off-hand,
25 Thrown, 26 Ranged Right (wands, also guns/crossbows on some items), 28 Relic.

**`OverallQualityID`**: 0 Poor, 1 Common, 2 Uncommon, 3 Rare, 4 Epic, 5 Legendary, 6 Artifact.

**`Bonding`**: 0 none, 1 BoP, 2 BoE, 3 BoU, 4 quest, 5 account.

**`AllowableClass`**: bitmask; `-1` or `32767` = all classes. Bit = `1 << (ChrClasses.ID - 1)`:
1 Warrior, 2 Paladin, 4 Hunter, 8 Rogue, 16 Priest, 64 Shaman, 128 Mage, 256 Warlock, 1024 Druid
(`ChrClasses.ID` 1,2,3,4,5,7,8,9,11 — 6 and 10 are unused in this build).
`ChrClasses.ArmorTypeMask` is the matching bitmask over armor `SubclassID` (Warrior 127 =
misc/cloth/leather/mail/plate/cosmetic/shield).

**`AllowableRace_0/1`**: `-1` = all. Only 7 equippables are race-restricted. Ignore.

**Faction** is not derivable for gear: the PvP sets (Grand Marshal's / High Warlord's, ...) carry
no `Flags_1` horde/alliance bit, no race mask and no `OppositeFactionItemID`; those bits and
columns are only set on mounts, pets and deprecated items. Exported items are for both factions.

**Unique / unique-equipped**: `MaxCount == 1` → "Unique". `Flags_0 & 0x80000` → "Unique-Equipped"
(86 items, mostly trinkets). `LimitCategory != 0` → unique-equipped group with
`ItemLimitCategory.Quantity`.

**`Flags_0`** (retail `ItemFlags`): `0x10` deprecated, `0x8000` no disenchant, `0x80000`
unique-equipped. **`Flags_1`** (retail `ItemFlags2`): `0x200` "caster weapon" is **never set** in
this build; `0x2000`/`0x4000` are vendor-price flags. Other flags do not affect the sim.

## 1.4 Stats

`ItemSparse` stores stats as up to 10 (`StatModifier_bonusStat_i`, `StatPercentEditor_i`) pairs;
`bonusStat = -1` means unused. The value is **not** stored; it is a percentage of the item's
budget in `RandPropPoints`:

```
group   = by InventoryType:
            0: Head(1) Chest(5) Legs(7) Robe(20) Two-Hand(17) Thrown(25)
            1: Shoulder(3) Waist(6) Feet(8) Hands(10) Trinket(12)
            2: Neck(2) Wrist(9) Finger(11) Shield(14) Back(16) Held-off-hand(23)
            3: One-Hand(13) Main Hand(21) Off Hand(22) Wand(26 with subclass 19)
            4: Ranged(15) Ranged Right(26 non-wand) Relic(28)
column  = Epic     for quality 4 / 5 / 6
          Superior for quality 3
          Good     for quality 0 / 1 / 2
budget  = RandPropPoints[ItemLevel].<column>_<group>
value_i = floor(StatPercentEditor_i * budget / 10000 + 0.5)
```

Worked examples. Thunderfury (ilvl 80, legendary, One-Hand → `Epic_3[80]` = 23): 2174 → 5 Agi,
3478 → 8 Sta, 3478 → 8 Fire Res, 3913 → 9 Nature Res. Lionheart Helm (ilvl 61, epic, Head →
`Epic_0[61]` = 45): 4000 → 18 Str, 6222 → 28 crit rating (= 2 %), 4444 → 20 hit rating (= 2 %).
The ranged group is a *fifth* column, vanilla style (`Epic_4[78]` = 17 for AQ40 bows).
Derived budgets for ~560 Classic items agreed with the table for every (ilvl, quality, slot).

**`bonusStat` ids** (retail `ItemModType` plus Classic-era extensions; only ids seen in the data):

| id | meaning | notes |
|---|---|---|
| 0 / 1 | Mana / Health | test items only |
| 3 4 5 6 7 | Agility, Strength, Intellect, Spirit, Stamina | |
| 12 | Defense | value = defense skill points (1:1 at level 60) |
| 13 | Dodge rating | 12 rating = 1 % at level 60 |
| 14 | Parry rating | 15 rating = 1 % |
| 15 | Block rating | 5 rating = 1 % |
| 31 | Hit rating | 10 rating = 1 % (melee **and** spell; ids 16–18 are unused) |
| 32 | Crit rating | 14 rating = 1 % (melee **and** spell; ids 19–21 unused) |
| 36 | Haste rating | 4 items, test/Forever only |
| 37 | Expertise (retail id) | Forever-only; Dwarven Tree Chopper had "+2 Two-Handed Axes" in Classic and carries 37 here — its Forever meaning is unconfirmed |
| 38 / 39 | Attack Power / Ranged Attack Power | |
| 41 | Healing done | |
| 42 | Spell damage done (all schools) | older items split 41/42 |
| 43 | Mana per 5 s | |
| 44 | Armor penetration rating | 1 Forever item |
| 45 | Spell power (damage **and** healing) | most "+X spell damage and healing" items |
| 46 | Health per 5 s | |
| 47 | Spell penetration | values in this build are ~0.3× the Classic tooltip numbers — verify in game |
| 48 | Block value | |
| 50 | Bonus armor | same number as `QualityModifier`; count it once (see 1.5) |
| 51 52 53 54 55 56 | Fire, Frost, Holy, Shadow, Nature, Arcane resistance | |
| 83 84 85 86 87 88 89 | Damage done: Physical ("+X weapon damage"), Holy, Fire, Nature, Frost, Shadow, Arcane | SpellSchool order; 83 verified on Might of Cenarius |
| 90–105 | Weapon skill, in the `ITEM_MOD_*` order of `GlobalStrings`: 90 Two-Handed Axes, 91 Two-Handed Maces, 92 Two-Handed Swords, 93 Axes, 94 Bows, 95 Crossbows, 96 Daggers, 97 Dual Wield, 98 Fist Weapons, 99 Guns, 100 Maces, 101 Polearms, 102 Staves, 103 Swords, 104 Thrown, 105 Wands | every observed id matches its carrier (90 Huge Thorium Battleaxe, 91 Servomechanic Sledgehammer, 92 Bladewind, 96 Death's Sting, 98 Punchy's Punchers, 103 a test sword chest) |
| 106–118 | Profession skills in the same order (Alchemy … Tailoring) | 112 Herbalism, 117 Fishing observed |
| 112 113 114 117 | Herbalism, Mining, Skinning, Fishing skill | |
| 124 | All resistances | Obsidian belts |
| 125–132 | Attack power vs Humanoid, Elemental, Demon, Undead, Dragonkin, Giant, Beast, Mechanical | `GlobalStrings` order; 127 128 131 132 observed |
| 133–140 | Spell damage vs the same creature types, same order | 135 136 observed |
| 119 121 | Unknown (only on "Spell Penetration" test staves) | |

`GlobalStrings` (`ITEM_MOD_*` tags) holds the display strings but not the numeric ids.

**Gear hit and crit are one stat each** for melee, ranged and spells (`Stats::apply_item_stat`):
hit and crit rating (31 / 32) and the `HIT_CHANCE` / `CRIT_CHANCE` item stats of the hand-written
data (`data/enchants.yaml`) raise all three. The tables agree: there is no spell-only rating, and
Forever's own equip and set bonus spells carry the melee and the spell aura together ("Increased
Hit Chance 01" 432639, the Tier 1 2P "Hit" bonuses 1300940…1300966, the reworked Zandalar signets
1219510 / 1219512: `MOD_HIT_CHANCE` + `MOD_SPELL_HIT_CHANCE`, `MOD_CRIT_PERCENT` +
`MOD_SPELL_CRIT_CHANCE`). `SPELL_HIT_CHANCE` / `SPELL_CRIT_CHANCE` stay **spell only**: they only
describe enchants whose table spell has the spell aura alone (Brilliant Wizard Oil 25113,
Presence of Sight 24156, Power of the Scourge 29468), as "Increased Critical Spell" 18382 / 18384
on the old sets. Equip spells apply the auras they have, so an old melee-only bonus ("Increased
Critical 1" 7597) stays melee only. Talents and buffs keep their own split the same way.

## 1.5 Armor

The per-slot multipliers come from `ArmorLocation` (keyed by `InventoryType`; `Clothmodifier`,
`Leathermodifier`, `Chainmodifier`, `Platemodifier` are identical per row, `Modifier` is the cloak
column). Robe (20) has no row — use Chest (5). They were first derived empirically from the
Classic items and then confirmed by the table:

```
shield:  armor = round(ItemArmorShield[ItemLevel].Quality_<q>)
other:   material = by SubclassID: 1 Cloth, 2 Leather, 3 Mail, 4 Plate   (cloaks use Cloth)
         base  = ItemArmorTotal[ItemLevel].<material> * ItemArmorQuality[ItemLevel].Qualitymod_<q> * slot
         slot  = ArmorLocation[InventoryType].<material>modifier   (cloak: ArmorLocation[16].Modifier)
               = Head .13  Shoulder .12  Chest/Robe .16  Waist .09  Legs .14  Feet .11  Wrist .07  Hands .10  Back .08
         armor = round(base) + bonus_armor
bonus_armor = ItemSparse.QualityModifier  (armor only; bonusStat 50, when present, is the
              same number — do not add both. 139 armor items have bonus armor without a
              bonusStat 50: Runic Plate, Volcanic, Sandstalker, ...)
```

Verified on 352 of ~360 Classic armor pieces (the rest are items Forever re-tuned).

## 1.6 Weapon damage

```
table = InventoryType 17                → ItemDamageTwoHand
        InventoryType 13 / 21 / 22      → ItemDamageOneHand
        InventoryType 15 / 25 / 26      → by SubclassID: 19 wand → ItemDamageWand
                                                         16 thrown → ItemDamageThrown
                                                         2 / 3 / 18 → ItemDamageRanged
dps   = table[ItemLevel].Quality_<OverallQualityID>          (float, do not round)
        * (1 + QualityModifier / 100)                        (weapons: a DPS percentage)
avg   = dps * ItemDelay / 1000
min   = floor(avg * (1 - DmgVariance / 2))
max   = floor(avg * (1 + DmgVariance / 2) + 0.5)
```

Exact for 70 of 72 matched Classic melee/ranged weapons (the two others changed speed in Forever).
`DamageType` (0 physical, 1–6 schools) is the school of the weapon's white damage; only a few
Forever weapons are non-physical. `ItemRange` is 100 for ranged weapons (irrelevant).

On a weapon `QualityModifier` is a DPS percentage, not bonus armor: Thunderfury −20 (its proc is
the upside; 65–122 here vs Classic 44–115), Benediction / Anathema −14, Grand Marshal's Stave −18,
the AQ20 caster weapons (Blade of Vaulted Secrets, Kris of Unspoken Names, …) −10, Atiesh −32.7.

**Open question — caster weapons.** In Classic, spell-damage weapons (Sharpened Silithid Femur,
Runesword of the Red, Jin'do's Judgement, Mindfang, …) have ~0.68–0.9× the DPS the formula gives.
Retail selects `ItemDamage*Caster` via `Flags_1 & 0x200`, but here that flag is never set and the
caster tables equal the melee tables, so nothing in the dump lowers their DPS. Either Forever gives
them full melee DPS or the client uses a criterion not in these tables. Irrelevant for the Warrior
milestone; check a caster dagger tooltip in game before relying on caster weapon damage.
Only a few caster weapons carry a negative `QualityModifier` (above); the rest (Runesword of the
Red, Sharpened Silithid Femur, Jin'do's Judgement, Mindfang) get full DPS from the tables.

## 1.7 Item spells: procs, on-use, on-equip

`ItemXItemEffect(ItemID → ItemEffectID)` then `ItemEffect`:

| `TriggerType` | meaning | count on equippables |
|---|---|---|
| 0 | On use (`CoolDownMSec`, `CategoryCoolDownMSec`, `SpellCategoryID` = shared cooldown group) | 631 |
| 1 | On equip (a passive aura, e.g. "Attack Power - Feral (+140)", "+X to Lockpicking") | 1 267 |
| 2 | Chance on hit (proc; the proc chance / PPM lives on the spell, see `SpellProcsPerMinute` / spell tables) | 445 |
| 5 | No-delay use | 1 |
| 6 | Learn spell (Forever "Engrave …" rune items) | 26 |

An item can have several effects (max 5). `Charges` -1 = unlimited, 0 = n/a. `LegacySlotIndex` is
the 0..4 slot of the old `ItemSparse.Spell*` arrays. `ChrSpecializationID` is always 0.
Thunderfury → `ItemEffect 101091` → trigger 2, spell 21992 "Thunderfury". Resolving what the spell
does goes through the spell tables (`INSTRUCTIONS.md` section 2, plus `SpellMisc` / `SpellAuraOptions` below).

Note that in this build "Equip: Improves your chance to hit by 1%"-style bonuses on Classic items
are **stats** (ids 31/32/…), not on-equip spells; only genuine auras use trigger 1.

## 1.8 Sets

`ItemSet.ItemID_0..16` lists the members (`ItemSparse.ItemSet` is the back-pointer).
`ItemSetSpell` rows with `ItemSetID` give the bonuses: `Threshold` = pieces required, `SpellID` →
the bonus spell. `ChrSpecID`/`TraitSubTreeID`/`SetFlags`/`RequiredSkill` are always 0.
Battlegear of Might = set 209: (3) 23562, (5) 21838, (8) 23561.

## 1.9 Random-suffix items ("of the Bear")

Vanilla's `ItemRandomSuffix` is replaced by the retail item-bonus system. 1 707 Classic items have an
`ItemXBonusTree` row. Such an item has **no base stats** in `ItemSparse`; each possible suffix is one
`ItemBonusTreeNode` (`ItemContext` 0) of the tree, whose `ChildItemBonusListID` groups `ItemBonus` rows:

- `Type 2` = stat: `Value_0` = bonusStat id, `Value_1` = percent — applied with the **same
  `RandPropPoints` formula as 1.4** (retail semantics; not verifiable against ClassicSim data, which
  has no suffix items).
- `Type 5` = name suffix: `Value_0` → `ItemNameDescription.ID` ("of the Eagle" 14300, "of the
  Bear" 14302, "of Stamina" 14311 …).

Trees 5654–5661 / 5838 / 5839 are the Classic suffix pools (24–41 suffixes each); trees 6130/6138
are Forever "Premier"/"Restored" gear (types 0/13/27/54 = upgrade/appearance bookkeeping, ignore).
A two-stat suffix has 6666 per stat, a one-stat suffix 10000.

## 1.10 What is missing from the dump

- **`ItemSparse` rows** for ~half the Classic equippables (see 1.1) — the important one.
- GameTables (`CombatRatings`, `ItemSocketCostPerLevel`) — rating→% factors derived above; these
  are `.txt` GameTables, not DB2s.
- `ItemRandomProperties` / `ItemRandomSuffix` — not needed, superseded by `ItemBonus`.
- Nothing else on the item side. `Spell` (36 789 rows: `NameSubtext_lang`, `Description_lang`,
  `AuraDescription_lang`) is present and gives human-readable tooltips for item spells; tokens are
  the client's: `$s1`/`$s2` = effect 1/2 base points, `$d` = duration, `$h` = proc chance,
  `$x1` = chain targets, `$<spellId>s1` = effect of another spell, `${…}` = formula, and
  `$<name>` variables come from `SpellDescriptionVariables` (93 rows of `$power=${…}` macros,
  linked from the spell). E.g. Hand of Justice 15600: "${$h/3}% chance on Melee hit to gain $s1
  extra attack." The sim itself reads `SpellEffect` / `SpellAuraOptions`, not the text.
- Present and sufficient for item procs / on-use effects: `SpellEffect`, `SpellName`, `SpellMisc`
  (attributes, `SchoolMask`, `CastingTimeIndex`, `DurationIndex` → `SpellDuration`, `RangeIndex`),
  `SpellAuraOptions` (keyed by `SpellID`: `ProcChance` %, `ProcCategoryRecovery` = internal
  cooldown ms, `ProcCharges`, `SpellProcsPerMinuteID` → `SpellProcsPerMinute.BaseProcRate`,
  `ProcTypeMask_0/1` = retail `ProcFlags`, `CumulativeAura` = max stacks), `SpellProcsPerMinute`,
  `SpellCooldowns`, `SpellDuration`, `SpellPower`, `SpellLevels`, `SpellClassOptions`,
  `SpellCategory`, `SpellLabel`. Example: Hand of Justice's aura 15600 → `SpellAuraOptions`
  `ProcChance` 3, `ProcCategoryRecovery` 2000, `ProcTypeMask_0` 20 (melee swing / melee ability).
  `ProcChance` 101 (Thunderfury 21992) means "always / handled by the effect", not 101 %.
