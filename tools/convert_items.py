#!/usr/bin/env python3
"""Converts the ClassicSim XML item database to the ClassicSimForever YAML schema.

Usage:
    python tools/convert_items.py <ClassicSim>/Equipment/EquipmentDb data

Reads every XML file listed in the source's `equipment_paths.xml` (plus `set_bonuses.xml`) and
writes one YAML file per XML file into `data/items/`, and `data/set_bonuses.yaml`.

Only the Python standard library is used; YAML is emitted by hand (strings are written as JSON
strings, which YAML accepts verbatim).
"""

import json
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

# XML attribute -> spec field for procs. `type` becomes `stat`.
PROC_ATTRS = [
    "display_name", "min", "max", "innate_threat", "duration", "tick_rate", "amount",
    "max_stacks", "value", "spell_dmg_coefficient", "dmg_over_duration",
]
PROC_SOURCE_ATTRS = {
    "proc_magic_hit": "magic_hit",
    "proc_melee_auto": "melee_auto",
    "proc_melee_skill": "melee_skill",
    "proc_melee_weapon_side": "melee_weapon_side",
    "proc_weapon_side": "melee_weapon_side",
    "proc_ranged_auto": "ranged_auto",
    "proc_ranged_skill": "ranged_skill",
}
INT_ATTRS = {"min", "max", "innate_threat", "duration", "amount", "max_stacks", "dmg_over_duration"}
FLOAT_ATTRS = {"tick_rate", "value", "spell_dmg_coefficient"}

# Stats whose set bonus values are stored as hundredths of a percent in the C++ data.
CHANCE_STATS = {"CRIT_CHANCE", "HIT_CHANCE", "SPELL_CRIT_CHANCE", "SPELL_HIT_CHANCE"}

# Item types that the C++ code did not classify but the Rust schema names explicitly.
TYPE_BY_SLOT = {"RELIC": "RELIC"}

SKIPPED_SLOTS = {"PROJECTILE", "QUIVER"}


def yaml_scalar(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        text = repr(value)
        return text if "." in text or "e" in text else text + ".0"
    return json.dumps(value, ensure_ascii=False)


def emit(lines, key, value, indent):
    """Emits `key: value` where value may be a scalar, list of scalars, dict or list of dicts."""
    pad = "  " * indent
    if isinstance(value, dict):
        if not value:
            return
        lines.append(f"{pad}{key}:")
        for sub_key, sub_value in value.items():
            emit(lines, sub_key, sub_value, indent + 1)
    elif isinstance(value, list):
        if not value:
            return
        if all(isinstance(v, dict) for v in value):
            lines.append(f"{pad}{key}:")
            for entry in value:
                first = True
                for sub_key, sub_value in entry.items():
                    if isinstance(sub_value, (dict, list)):
                        sub_lines = []
                        emit(sub_lines, sub_key, sub_value, indent + 2)
                        if not sub_lines:
                            continue
                        if first:
                            sub_lines[0] = f"{pad}  - " + sub_lines[0].lstrip()
                            first = False
                        lines.extend(sub_lines)
                    else:
                        prefix = f"{pad}  - " if first else f"{pad}    "
                        lines.append(f"{prefix}{sub_key}: {yaml_scalar(sub_value)}")
                        first = False
        else:
            lines.append(f"{pad}{key}: [" + ", ".join(yaml_scalar(v) for v in value) + "]")
    else:
        lines.append(f"{pad}{key}: {yaml_scalar(value)}")


def number(text):
    try:
        return int(text)
    except ValueError:
        return float(text)


def yes_no(text):
    return text.strip().lower() == "yes"


def convert_proc(element):
    attrs = element.attrib
    proc = {"name": attrs["name"], "rate": float(attrs["rate"])}
    if "internal_cd" in attrs and float(attrs["internal_cd"]) != 0:
        proc["internal_cd"] = float(attrs["internal_cd"])
    for attr in PROC_ATTRS:
        if attr in attrs:
            text = attrs[attr]
            if attr in INT_ATTRS:
                proc[attr] = int(float(text))
            elif attr in FLOAT_ATTRS:
                proc[attr] = float(text)
            else:
                proc[attr] = text
    if "instant" in attrs:
        proc["instant"] = yes_no(attrs["instant"])
    if "type" in attrs:
        proc["stat"] = attrs["type"]
    sources = {}
    for attr, flag in PROC_SOURCE_ATTRS.items():
        if attr in attrs and attrs[attr].lower() == "true":
            sources[flag] = True
    if sources:
        proc["sources"] = sources
    return proc


def convert_use(element):
    attrs = element.attrib
    use = {"name": attrs["name"]}
    if "cooldown" in attrs:
        use["cooldown"] = int(attrs["cooldown"])
    if "type" in attrs:
        use["stat"] = attrs["type"]
    if "value" in attrs:
        use["value"] = number(attrs["value"])
    if "duration" in attrs:
        use["duration"] = int(attrs["duration"])
    return use


def convert_item(element):
    info = element.find("info").attrib
    slot = info["slot"]
    if slot in SKIPPED_SLOTS:
        return None

    item = {
        "id": int(element.attrib["id"]),
        "name": info["name"],
        "phase": int(element.attrib["phase"]),
        "slot": slot,
        "type": TYPE_BY_SLOT.get(slot, info["type"]),
        "quality": info["quality"],
    }
    if yes_no(info.get("unique", "no")):
        item["unique"] = True
    if "req_lvl" in info:
        item["req_lvl"] = int(info["req_lvl"])
    if "item_lvl" in info:
        item["item_lvl"] = int(info["item_lvl"])
    if yes_no(info.get("boe", "no")):
        item["boe"] = True
    if "icon" in info:
        item["icon"] = info["icon"]
    if info.get("faction"):
        item["faction"] = info["faction"]

    classes = [c.attrib["class"] for c in element.findall("class_restriction")]
    if classes:
        item["class_restrictions"] = classes

    dmg = element.find("dmg_range")
    if dmg is not None:
        item["damage"] = {
            "min": int(dmg.attrib["min"]),
            "max": int(dmg.attrib["max"]),
            "speed": float(dmg.attrib["speed"]),
        }

    stats = {}
    stats_element = element.find("stats")
    if stats_element is not None:
        for stat in stats_element.findall("stat"):
            key = stat.attrib["type"]
            stats[key] = stats.get(key, 0) + number(stat.attrib["value"])
    if stats:
        item["stats"] = dict(sorted(stats.items()))

    procs = [convert_proc(spell) for proc in element.findall("proc") for spell in proc.findall("spell")]
    if procs:
        item["procs"] = procs

    uses = [convert_use(use) for uses in element.findall("uses") for use in uses.findall("use")]
    if uses:
        item["uses"] = uses

    modifies = [m.attrib["name"] for m in element.findall("modifies") if "name" in m.attrib]
    if modifies:
        item["modifies"] = modifies

    mutex = [int(m.attrib["item_id"]) for m in element.findall("mutex") if "item_id" in m.attrib]
    if mutex:
        item["mutex"] = mutex

    affixes = [int(a.attrib["id"]) for ra in element.findall("random_affixes") for a in ra.findall("affix")]
    if affixes:
        item["random_affixes"] = affixes

    effects = [" ".join(e.text.split()) for e in element.findall("special_equip_effect") if e.text]
    if effects:
        item["special_equip_effects"] = effects

    source = element.find("source")
    if source is not None and source.text and source.text.strip():
        item["source"] = " ".join(source.text.split())

    flavour = element.find("flavour_text")
    if flavour is not None and flavour.text and flavour.text.strip():
        item["flavour_text"] = " ".join(flavour.text.split())

    return item


def write_yaml_list(path, entries, header):
    lines = [f"# {line}" for line in header] + [""]
    for entry in entries:
        first = True
        for key, value in entry.items():
            sub_lines = []
            emit(sub_lines, key, value, 1)
            if not sub_lines:
                continue
            if first:
                sub_lines[0] = "- " + sub_lines[0].lstrip()
                first = False
            lines.extend(sub_lines)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def convert_item_files(source_root, items_dir):
    paths_file = source_root / "equipment_paths.xml"
    root = ET.parse(paths_file).getroot()
    total = 0
    for file_element in root.findall("file"):
        relative = file_element.attrib["path"]
        # equipment_paths.xml refers to the deploy layout (Items/...); the repo has them next to it.
        xml_path = source_root / relative.removeprefix("Items/")
        if not xml_path.exists():
            print(f"skipping missing {xml_path}", file=sys.stderr)
            continue
        items = []
        for element in ET.parse(xml_path).getroot():
            item = convert_item(element)
            if item is not None:
                items.append(item)
        if not items:
            continue
        stem = "_".join(part.lower() for part in Path(relative.removeprefix("Items/")).with_suffix("").parts)
        out_path = items_dir / f"{stem}.yaml"
        write_yaml_list(out_path, items, [f"Generated from ClassicSim {relative} by tools/convert_items.py."])
        total += len(items)
        print(f"{out_path}: {len(items)} items")
    return total


def convert_set_bonuses(source_root, out_path):
    root = ET.parse(source_root / "set_bonuses.xml").getroot()
    sets = []
    for set_element in root.findall("set"):
        entry = {
            "name": set_element.attrib["name"],
            "items": [int(i.attrib["value"]) for i in set_element.findall("item_id")],
        }
        bonuses = []
        for bonus in set_element.findall("bonus"):
            record = {"pieces": int(bonus.attrib["value"])}
            if bonus.text and bonus.text.strip():
                record["description"] = " ".join(bonus.text.split())
            if "item_stat" in bonus.attrib and "stat_value" in bonus.attrib:
                stat = bonus.attrib["item_stat"]
                value = number(bonus.attrib["stat_value"])
                if stat in CHANCE_STATS:
                    value = value / 10000
                record["stat"] = stat
                record["value"] = value
            bonuses.append(record)
        if bonuses:
            entry["bonuses"] = bonuses
        sets.append(entry)
    write_yaml_list(out_path, sets, ["Generated from ClassicSim set_bonuses.xml by tools/convert_items.py."])
    print(f"{out_path}: {len(sets)} sets")


def main():
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(2)
    source_root = Path(sys.argv[1])
    data_dir = Path(sys.argv[2])
    items_dir = data_dir / "items"
    items_dir.mkdir(parents=True, exist_ok=True)
    total = convert_item_files(source_root, items_dir)
    convert_set_bonuses(source_root, data_dir / "set_bonuses.yaml")
    print(f"{total} items converted")


if __name__ == "__main__":
    main()
