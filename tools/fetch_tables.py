"""Downloads the latest client table dump from wago.tools into `data/tables/`.

Run from anywhere: `python tools/fetch_tables.py [options] [Table ...]`. Asks
https://wago.tools/api/builds/<product>/latest for the newest build of the product
(`wow_classic_beta` by default), then fetches every table as
`data/tables/<Table>.<build>.csv` from https://wago.tools/db2/<Table>/csv?build=<build>.

It fetches the tables listed in `TABLES` below plus any other table already present in
`data/tables/` (any build), so an empty directory can be filled and a refresh keeps extra
tables; names given on the command line are fetched in addition. Once every
table is downloaded, the files of the other builds are removed (`TableDir::open` wants a single
build in the directory) unless `--keep-old` is given. Nothing is removed if a download fails.

Then the build's server-side hotfixes are applied (`tools/fetch_hotfixes.py`: downloaded once into
`data/tables/hotfixes.<build>.json`, `--refresh-hotfixes` downloads them again; the raw tables are
kept in `data/tables/raw/`). `--no-hotfixes` leaves the raw tables in place.

Only the standard library is used.
"""
import argparse
import concurrent.futures
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

import fetch_hotfixes

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, ".."))
DEFAULT_DIR = os.path.join(ROOT, "data", "tables")

API = "https://wago.tools/api"
DB2 = "https://wago.tools/db2"
USER_AGENT = "ClassicSimForever-fetch-tables/1.0"

# The tables the csim-tables exports read, so an empty directory can be filled.
TABLES = [
    "ArmorLocation", "CharBaseInfo", "CharBaseSection", "ChrClasses", "ChrClassesXPowerTypes",
    "ChrRaceRacialAbility", "ChrRaces", "ChrRacesCreateScreenIcon", "ChrSpecialization",
    "CooldownSet", "CooldownSetLinkedSpell", "CooldownSetSpell", "Curve", "CurvePoint",
    "Faction", "FactionGroup", "GlobalStrings", "Item", "ItemArmorQuality", "ItemArmorShield",
    "ItemArmorTotal", "ItemBonus", "ItemBonusList", "ItemBonusListGroup", "ItemBonusTree",
    "ItemBonusTreeNode", "ItemClass", "ItemCondition", "ItemDamageAmmo", "ItemDamageOneHand",
    "ItemDamageOneHandCaster", "ItemDamageRanged", "ItemDamageThrown", "ItemDamageTwoHand",
    "ItemDamageTwoHandCaster", "ItemDamageWand", "ItemEffect", "ItemExtendedCost",
    "ItemLimitCategory", "ItemLimitCategoryCondition", "ItemNameDescription", "ItemSearchName",
    "ItemSet", "ItemSetSpell", "ItemSparse", "ItemSpecOverride", "ItemSubClass",
    "ItemSubClassMask", "ItemXBonusTree", "ItemXItemEffect", "PlayerCondition",
    "PlayerExpectedStat", "PowerDisplay", "PowerType", "RaceStat", "RandPropPoints",
    "Resistances", "SkillLine", "SkillLineAbility", "SkillLineCategory", "SkillLineXTraitTree",
    "SkillRaceClassInfo", "Spell", "SpellAuraOptions", "SpellAuraRestrictions",
    "SpellCastTimes", "SpellCategories", "SpellCategory", "SpellClassOptions",
    "SpellCooldowns", "SpellDescriptionVariables", "SpellDuration", "SpellEffect",
    "SpellEquippedItems", "SpellInterrupts", "SpellItemEnchantment", "SpellLabel",
    "SpellLevels", "SpellMisc", "SpellName", "SpellPower", "SpellProcsPerMinute",
    "SpellRadius", "SpellRange", "SpellReagents", "SpellShapeshift", "SpellShapeshiftForm",
    "SpellTargetRestrictions", "SpellTotems", "SpellXSpellVisual", "Talent", "TalentTab",
    "TotemCategory", "TraitCond", "TraitCost", "TraitCurrency", "TraitCurrencySource",
    "TraitDefinition", "TraitDefinitionEffectPoints", "TraitEdge", "TraitNode",
    "TraitNodeEntry", "TraitNodeGroup", "TraitNodeGroupDisplayInfo",
    "TraitNodeGroupXTraitCond", "TraitNodeGroupXTraitNode", "TraitNodeXTraitCond",
    "TraitNodeXTraitNodeEntry", "TraitSystem", "TraitTree", "TraitTreeXTraitCost",
    "TraitTreeXTraitCurrency",
]

# `<Table>.<build>.csv`, the same split as crates/csim-tables/src/dir.rs.
FILE_NAME = re.compile(r"^([A-Za-z0-9_]+)\.(\d+(?:\.\d+)*)\.csv$")


def get(url, retries=3):
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    for attempt in range(1, retries + 1):
        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                return resp.read()
        except urllib.error.HTTPError as e:
            # A 404 will not go away by asking again.
            if e.code == 404 or attempt == retries:
                raise
        except (urllib.error.URLError, TimeoutError):
            if attempt == retries:
                raise
        time.sleep(2 * attempt)


def latest_build(product):
    data = json.loads(get(f"{API}/builds/{urllib.parse.quote(product)}/latest"))
    return data["version"]


def local_files(directory):
    """Maps each build found in `directory` to its {table: file name}."""
    builds = {}
    if os.path.isdir(directory):
        for name in os.listdir(directory):
            m = FILE_NAME.match(name)
            if m:
                builds.setdefault(m.group(2), {})[m.group(1)] = name
    return builds


def fetch(table, build, directory, force):
    path = os.path.join(directory, f"{table}.{build}.csv")
    if os.path.exists(path) and not force:
        return table, "present"
    query = urllib.parse.urlencode({"build": build})
    body = get(f"{DB2}/{urllib.parse.quote(table)}/csv?{query}")
    if body.lstrip().startswith(b"<"):
        raise ValueError("got an HTML page instead of a CSV table")
    tmp = path + ".part"
    with open(tmp, "wb") as f:
        f.write(body)
    os.replace(tmp, path)
    return table, f"{len(body):,} bytes"


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("tables", nargs="*", help="extra tables to fetch besides the known ones")
    p.add_argument("--product", default="wow_classic_beta",
                   help="wago.tools product (default: %(default)s)")
    p.add_argument("--build", help="fetch this build instead of the product's latest")
    p.add_argument("--dir", default=DEFAULT_DIR, help="table directory (default: data/tables)")
    p.add_argument("--keep-old", action="store_true", help="keep the files of other builds")
    p.add_argument("--force", action="store_true", help="re-download files that already exist")
    p.add_argument("--jobs", type=int, default=4, help="parallel downloads (default: %(default)s)")
    p.add_argument("--dry-run", action="store_true", help="print the plan, download nothing")
    p.add_argument("--no-hotfixes", action="store_true",
                   help="keep the raw tables, without the server-side hotfixes")
    p.add_argument("--refresh-hotfixes", action="store_true",
                   help="download the hotfixes again over their cache")
    args = p.parse_args()

    builds = local_files(args.dir)
    local = {t for files in builds.values() for t in files}
    tables = sorted(set(TABLES) | local | set(args.tables))

    build = args.build or latest_build(args.product)
    print(f"{args.product}: build {build}; local builds: {', '.join(sorted(builds)) or 'none'}")
    print(f"{len(tables)} tables -> {args.dir}")
    if args.dry_run:
        print(" ".join(tables))
        return

    os.makedirs(args.dir, exist_ok=True)
    failed = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        jobs = {pool.submit(fetch, t, build, args.dir, args.force): t for t in tables}
        for done, future in enumerate(concurrent.futures.as_completed(jobs), 1):
            table = jobs[future]
            try:
                _, status = future.result()
                print(f"[{done}/{len(tables)}] {table}: {status}")
            except Exception as e:  # noqa: BLE001 - report every failure, then stop
                print(f"[{done}/{len(tables)}] {table}: FAILED ({e})", file=sys.stderr)
                failed.append(table)

    if failed:
        sys.exit(f"{len(failed)} tables failed: {' '.join(sorted(failed))}; nothing removed")

    if not args.keep_old:
        removed = 0
        for old, files in builds.items():
            if old == build:
                continue
            raw = os.path.join(args.dir, fetch_hotfixes.RAW_DIR)
            for name in files.values():
                os.remove(os.path.join(args.dir, name))
                removed += 1
                if os.path.exists(os.path.join(raw, name)):
                    os.remove(os.path.join(raw, name))
            cache = fetch_hotfixes.cache_path(args.dir, old)
            if os.path.exists(cache):
                os.remove(cache)
        if removed:
            print(f"removed {removed} files of builds {', '.join(b for b in builds if b != build)}")
    if args.no_hotfixes:
        fetch_hotfixes.restore_raw(args.dir, build)
    else:
        try:
            hotfixes = fetch_hotfixes.fetch_hotfixes(args.dir, build, jobs=args.jobs,
                                                     force=args.refresh_hotfixes)
        except (fetch_hotfixes.PageError, urllib.error.URLError) as e:
            sys.exit(f"hotfixes: {e}; the tables are raw, --no-hotfixes to accept that")
        fetch_hotfixes.print_spells(hotfixes)
        fetch_hotfixes.apply_hotfixes(args.dir, build, hotfixes)
    print("done; re-run the exports in data/README.md ('Re-exporting from a new dump')")


if __name__ == "__main__":
    main()
