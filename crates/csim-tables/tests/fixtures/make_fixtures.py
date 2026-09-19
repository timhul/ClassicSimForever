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
keep("PowerType", lambda get: True)
