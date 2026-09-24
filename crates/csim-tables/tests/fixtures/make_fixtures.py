"""Extracts a tiny, self-contained fixture set from the real table dump for the csim-tables tests.

Run from anywhere: `python crates/csim-tables/tests/fixtures/make_fixtures.py [build]`. Reads
`data/tables/<Table>.<build>.csv` and writes the rows of a handful of Warrior spells and talents
to `crates/csim-tables/tests/fixtures/tables/`. Re-run it after changing the spell / node sets
below or after a new dump; the tests in `tests/tables.rs` assert on the values of these rows.
"""
import csv
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
SRC = os.path.join(ROOT, "data", "tables")
DST = os.path.join(HERE, "tables")
V = sys.argv[1] if len(sys.argv) > 1 else "1.60.1.69893"

SPELLS = {12294, 12834, 12162, 412609, 12319, 12966, 78, 284, 2458, 7381, 11574, 2687, 29131,
          12282, 20572, 12292, 12286, 5308, 26651, 1680, 25288, 355, 694, 5246, 20511}
NODES = {105941, 105945, 105950, 105956}
SKILL_LINES = {26, 256, 257, 125}
CLASSES = {1}
RACES = {1, 2}


def load(name):
    with open(os.path.join(SRC, f"{name}.{V}.csv"), encoding="utf-8", newline="") as f:
        r = csv.reader(f)
        header = next(r)
        return header, list(r)


def write(name, header, rows):
    os.makedirs(DST, exist_ok=True)
    with open(os.path.join(DST, f"{name}.{V}.csv"), "w", encoding="utf-8", newline="") as f:
        w = csv.writer(f, lineterminator="\n")
        w.writerow(header)
        w.writerows(rows)
    print(f"{name}: {len(rows)} rows")


def keep(name, pred):
    header, rows = load(name)
    idx = {c: i for i, c in enumerate(header)}
    kept = [r for r in rows if pred(lambda c: r[idx[c]])]
    write(name, header, kept)
    return header, kept, idx


def in_set(col, s):
    return lambda get: int(get(col)) in s


# --- spells ------------------------------------------------------------------------------
keep("SpellName", in_set("ID", SPELLS))
keep("Spell", in_set("ID", SPELLS))
_, misc, midx = keep("SpellMisc", in_set("SpellID", SPELLS))
keep("SpellEffect", in_set("SpellID", SPELLS))
keep("SpellPower", in_set("SpellID", SPELLS))
keep("SpellCooldowns", in_set("SpellID", SPELLS))
_, cats, cidx = keep("SpellCategories", in_set("SpellID", SPELLS))
keep("SpellCategory", in_set("ID", {int(r[cidx["Category"]]) for r in cats} | {133}))
keep("SpellDuration", in_set("ID", {int(r[midx["DurationIndex"]]) for r in misc}))
keep("SpellCastTimes", in_set("ID", {int(r[midx["CastingTimeIndex"]]) for r in misc}))
keep("SpellRange", in_set("ID", {int(r[midx["RangeIndex"]]) for r in misc}))
keep("SpellRadius", in_set("ID", {9, 14}))
keep("SpellLevels", in_set("SpellID", SPELLS))
keep("SpellAuraOptions", in_set("SpellID", SPELLS))
keep("SpellProcsPerMinute", lambda get: True)
keep("SpellClassOptions", in_set("SpellID", SPELLS))
keep("SpellShapeshift", in_set("SpellID", SPELLS))
keep("SpellShapeshiftForm", in_set("ID", {17, 18, 19}))
keep("SpellAuraRestrictions", in_set("SpellID", SPELLS))
keep("SpellEquippedItems", in_set("SpellID", SPELLS))
keep("SpellTargetRestrictions", in_set("SpellID", SPELLS))
keep("SpellLabel", in_set("SpellID", SPELLS))

# --- skill lines -------------------------------------------------------------------------
keep("SkillLine", in_set("ID", SKILL_LINES))
keep("SkillLineAbility", lambda get: int(get("SkillLine")) in SKILL_LINES and int(get("Spell")) in SPELLS)
keep("SkillRaceClassInfo", in_set("SkillID", SKILL_LINES))
keep("SkillLineXTraitTree", in_set("SkillLineID", SKILL_LINES))

# --- traits ------------------------------------------------------------------------------
keep("TraitTree", in_set("ID", {1117}))
keep("TraitNode", in_set("ID", NODES))
_, nxe, nidx = keep("TraitNodeXTraitNodeEntry", in_set("TraitNodeID", NODES))
entries = {int(r[nidx["TraitNodeEntryID"]]) for r in nxe}
_, ents, eidx = keep("TraitNodeEntry", in_set("ID", entries))
defs = {int(r[eidx["TraitDefinitionID"]]) for r in ents}
keep("TraitDefinition", in_set("ID", defs))
_, eps, pidx = keep("TraitDefinitionEffectPoints", in_set("TraitDefinitionID", defs))
curves = {int(r[pidx["CurveID"]]) for r in eps}
keep("Curve", in_set("ID", curves))
keep("CurvePoint", in_set("CurveID", curves))
keep("TraitEdge", lambda get: int(get("LeftTraitNodeID")) in NODES and int(get("RightTraitNodeID")) in NODES)
_, gxn, gidx = keep("TraitNodeGroupXTraitNode", in_set("TraitNodeID", NODES))
groups = {int(r[gidx["TraitNodeGroupID"]]) for r in gxn}
keep("TraitNodeGroup", in_set("ID", groups))
_, gxc, cxidx = keep("TraitNodeGroupXTraitCond", in_set("TraitNodeGroupID", groups))
conds = {int(r[cxidx["TraitCondID"]]) for r in gxc}
keep("TraitCond", in_set("ID", conds))
keep("TraitNodeGroupDisplayInfo", in_set("TraitNodeGroupID", groups))
keep("TraitCurrency", in_set("ID", {3820}))
keep("TraitCurrencySource", lambda get: int(get("TraitCurrencyID")) == 3820 and int(get("PlayerLevel")) in (10, 11, 60))

# --- chr ---------------------------------------------------------------------------------
keep("ChrClasses", in_set("ID", CLASSES))
keep("ChrRaces", in_set("ID", RACES))
keep("PlayerExpectedStat", lambda get: int(get("ClassID")) in CLASSES and int(get("Level")) in (1, 60))
keep("CharBaseInfo", lambda get: int(get("ClassID")) in CLASSES and int(get("RaceID")) in RACES)
keep("PowerType", lambda get: True)

# --- items -------------------------------------------------------------------------------
# Thunderfury, Lionheart Helm, Huhuran's Stinger (AQ40 bow), Earthstrike (on-use), Drake Fang
# Talisman, Arena Master (limit category), Wild Leather Shoulders (random suffix), Force Reactive
# Disk (shield, two on-equip effects), High Warlord's Greatsword, Zandalar Vindicator's Breastplate (set 474),
# Chromatic Cloak, Assassin's Throwing Axe, Conqueror's Battlegear (set 496) and Hand of Justice
# (no ItemSparse row in this dump). Battlegear of Might (set 209) has no member with a sparse row.
# For the derivation: Arena Grand Master (Rare, limit category), Libram of Fervor (relic), Tome of
# Arcane Domination (held off-hand), Sandstalker Breastplate (bonus armor), Green Lens (base stats
# and a suffix pool) and the deprecated Thunderfury copy.
ITEMS = {19019, 12640, 21616, 21180, 19406, 18706, 8210, 18168, 18877, 19822, 18509, 21135,
         21331, 21329, 21333, 21332, 21330, 11815,
         19024, 23203, 19308, 20478, 10504, 17802}
keep("Item", in_set("ID", ITEMS))
_, sparse, sidx = keep("ItemSparse", in_set("ID", ITEMS))
keep("ItemSubClass", lambda get: True)
for table in ("RandPropPoints", "ItemArmorTotal", "ItemArmorQuality", "ItemArmorShield",
              "ArmorLocation", "ItemDamageOneHand", "ItemDamageTwoHand", "ItemDamageRanged",
              "ItemDamageWand", "ItemDamageThrown"):
    keep(table, lambda get: True)
_, ixe, xidx = keep("ItemXItemEffect", in_set("ItemID", ITEMS))
keep("ItemEffect", in_set("ID", {int(r[xidx["ItemEffectID"]]) for r in ixe}))
sets = {int(r[sidx["ItemSet"]]) for r in sparse} - {0} | {209}
keep("ItemSet", in_set("ID", sets))
keep("ItemSetSpell", in_set("ItemSetID", sets))
keep("ItemLimitCategory", in_set("ID", {int(r[sidx["LimitCategory"]]) for r in sparse}))
_, ixt, tidx = keep("ItemXBonusTree", in_set("ItemID", ITEMS))
trees = {int(r[tidx["ItemBonusTreeID"]]) for r in ixt}
_, tnodes, nidx = keep("ItemBonusTreeNode", in_set("ParentItemBonusTreeID", trees))
_, bonuses, bidx = keep("ItemBonus", in_set("ParentItemBonusListID",
                                            {int(r[nidx["ChildItemBonusListID"]]) for r in tnodes}))
names = {int(r[sidx["ItemNameDescriptionID"]]) for r in sparse}
names |= {int(r[bidx["Value_0"]]) for r in bonuses if r[bidx["Type"]] == "5"}
keep("ItemNameDescription", in_set("ID", names))
# The item spells and set bonuses only need to exist (SpellName); the Spell* rows stay Warrior only.
header, rows = load("ItemEffect")
eidx = {c: i for i, c in enumerate(header)}
effect_ids = {int(r[xidx["ItemEffectID"]]) for r in ixe}
item_spells = {int(r[eidx["SpellID"]]) for r in rows if int(r[eidx["ID"]]) in effect_ids}
header, rows = load("ItemSetSpell")
sidx2 = {c: i for i, c in enumerate(header)}
item_spells |= {int(r[sidx2["SpellID"]]) for r in rows if int(r[sidx2["ItemSetID"]]) in sets}
keep("SpellName", in_set("ID", SPELLS | item_spells))
