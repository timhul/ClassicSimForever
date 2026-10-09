# `data/` — what the simulator reads

Everything the engine knows about the game is data: exported from the client table dumps where
the tables have it, hand-written where they do not. Nothing about a specific spell is hardcoded
in Rust; adding or updating spells means re-exporting from a new dump and, occasionally, adding
an override.

```
data/
├── tables/               DB2 → CSV dumps, one file per table and build, with the build's
│                         server-side hotfixes applied (gitignored, see below); raw/ holds
│                         the dumps as downloaded, hotfixes.<build>.json the hotfixes, and
│                         listfile-icons.csv the icon texture names by FileDataID
├── spells/
│   ├── warrior.yaml      generated: the Warrior spellbook, talents, runes and their payloads
│   ├── paladin.yaml      generated: the Paladin spellbook, talents, runes and their payloads
│   ├── rogue.yaml        generated: the Rogue spellbook, talents, runes and their payloads
│   ├── racials.yaml      generated: the racial abilities of every race
│   ├── externals.yaml    generated: the aura spells of the external buffs (`learnable: false`)
│   ├── enchants.yaml     generated: the spells the enchant procs name
│   ├── items.yaml        generated: the spells the items and set bonuses grant
│   └── overrides/
│       ├── warrior.yaml  hand-written: what the tables do not say (scripts, threat, sim flags)
│       ├── paladin.yaml  also the Paladin's server-side numbers and their sources (header)
│       ├── rogue.yaml
│       ├── racials.yaml
│       ├── externals.yaml
│       ├── enchants.yaml
│       ├── items.yaml    also the chance-on-hit rates of weapons (server data, not in the tables)
│       └── discard.txt   the effects the exporter drops (see "Pruning")
├── talents/
│   ├── warrior.yaml      generated: the Warrior talent tree (tabs, tiers, prerequisites, rank values)
│   ├── paladin.yaml      generated: the Paladin talent tree
│   └── rogue.yaml        generated: the Rogue talent tree
├── external_buffs.yaml   hand-written: the raid buffs, consumables and target debuffs other
│                         players provide — name, aura spell id, faction, classes, mutex, stacks
├── items/
│   └── <slot>.yaml       generated: the weapons and armor of quality Rare+ (one file per slot)
├── item_sets.yaml        generated: the item sets of the exported items and their bonus spells
├── enchants.yaml         hand-written: the enchants
├── races.yaml            hand-written: ids/factions from ChrRaces, base attributes (racials are spells)
├── classes/<class>.yaml  hand-written: stat rules from ChrClasses / PlayerExpectedStat, races from
│                         CharBaseInfo, base stats, proficiencies, enchant lists per slot
├── rotations/<class>/    hand-written: the rotations (precombat actions, ordered cast_if
│                         executors with conditions), see rotations/README.md; the three
│                         Warrior and five Rogue rotations are ports of the ClassicSim XML
│                         files, the three Paladin rotations are new (ClassicSim had no
│                         Twist of Light, and its Judgement consumed the seal)
├── characters/           hand-written: character setups for `csim run` (class, race, talents,
│                         gear, buffs, rotation, target); common/ holds the shared parts
│                         (buffs, talent builds, gear per faction) they include
├── keybinds/             hand-written: keys for playing a setup from the keyboard in csim-live
│                         (dw_fury, combat, ret)
├── sweeps/               hand-written: `csim sweep` files (last talent points, profiles)
├── raids/                hand-written: raid setups for `csim run --raid`, up to 8 parties of 5
│                         (counting the player) listing setups of characters/ by file name
├── SPELL_INSTRUCTIONS.md how the Spell* / SkillLine* / Trait* tables fit together
├── TALENT_INSTRUCTIONS.md
└── ITEM_INSTRUCTIONS.md
```

## Pipeline

```
data/tables/<Table>.<build>.csv
        │  csim-tables (crates/csim-tables): loads the tables, walks the class
        ▼
csim-tables export-spells --class warrior   →  data/spells/warrior.yaml
csim-tables export-spells --class paladin   →  data/spells/paladin.yaml
csim-tables export-spells --class rogue     →  data/spells/rogue.yaml
csim-tables export-spells --racials         →  data/spells/racials.yaml
csim-tables export-spells --externals       →  data/spells/externals.yaml  (ids from external_buffs.yaml + the rulesets)
csim-tables export-spells --enchants        →  data/spells/enchants.yaml   (ids the enchant procs name, the poisons' SpellItemEnchantment rows)
csim-tables export-talents --class warrior  →  data/talents/warrior.yaml
csim-tables export-talents --class paladin  →  data/talents/paladin.yaml
csim-tables export-talents --class rogue    →  data/talents/rogue.yaml
csim-tables export-items                    →  data/items/<slot>.yaml, data/item_sets.yaml
csim-tables export-spells --items           →  data/spells/items.yaml      (item and set bonus spells)
csim-tables export-all                      →  all of the above, then check
        │  + data/spells/overrides/*.yaml
        ▼
csim_engine::spell::record::SpellDb::load("data/spells")   (the engine)
csim_engine::talent::TalentDb::load("data/talents")
```

1. **`csim-tables`** reads the CSV dumps (`TableDir` finds the build from the file names,
   `Tables` indexes the rows) and walks a class: its skill lines (`SkillLineAbility`), its
   talent tree (`Trait*`), the payload spells those reach through `EffectTriggerSpell`,
   `OVERRIDE_ACTIONBAR_SPELLS`, `SpellAuraRestrictions` and the overrides' references
   (`SPELL_INSTRUCTIONS.md` §1.3–1.6). Each spell becomes one `SpellRecord`: the joined
   `Spell*` rows in snake_case (§1.7), effects included. The external buffs are walked the
   same way from the aura spell ids `data/external_buffs.yaml` names (Greater Blessing of
   Kings 25898, Faerie Fire 9907, …); ids another spell file already carries (the Warrior's
   Sunder Armor and Battle Shout) are not repeated, since the engine loads every file.
2. The walk is **pruned** (§1.10): effects the simulator has no use for are dropped, spells left
   with nothing to do are left out.
3. The generated files are committed so the engine and its tests never need the dump.
4. The engine loads the generated files plus the **overrides** and checks the cross references
   (`SpellDb::load`). `Spell`, `Buff`, `Effect`, `Proc` and `Periodic` are built from the
   records at runtime (`crates/csim-engine/src/spell/`).
5. **Talents** are a second, small walk (`TALENT_INSTRUCTIONS.md` §1.2–1.5): the class's Trait
   tree gives one entry per node — its spell, tab, tier, column, rank count, prerequisite and
   the `CurvePoint` value of each effect per rank (`data/talents/<class>.yaml`, schema
   `crates/csim-engine/src/talent/spec.rs`). What a talent *does* is not repeated there: it is
   the talent spell in `data/spells/<class>.yaml`, and the runtime applies rank *r* by
   substituting `rank_values[index][r − 1]` for the effect's base points and enabling the spell
   (stat auras, modifiers, procs, scripts and granted abilities all go through the spell
   machinery).

## Re-exporting from a new dump

1. Drop the CSV files in `data/tables/` (`<Table>.<build>.csv`; `csim-tables info` lists what
   is there and what is missing).
   `python tools/fetch_tables.py` downloads them, then applies the build's server-side
   hotfixes. The dumps are the client's `.db2` files, but Blizzard also changes rows on the
   server (new talents, spell values, whole items: the 70205 Fury rework and 4,581 item rows
   exist only as hotfixes), and the game and Wowhead have them. `tools/fetch_hotfixes.py`
   scrapes them from https://wago.tools/hotfixes (every row of the build, region and locale)
   into `data/tables/hotfixes.<build>.json`, keeps each hotfixed table's download in
   `data/tables/raw/` and rebuilds the table from it: a hotfix adds or replaces a record by its
   ID or removes it. It prints the hotfixed spell names and the hotfixed tables that are not
   fetched. Blizzard keeps pushing hotfixes to the same build: `python tools/fetch_hotfixes.py
   --force` (or `fetch_tables.py --refresh-hotfixes`) downloads them again and re-applies them;
   `--no-apply` (`--no-hotfixes`) restores the raw tables. A new build has no hotfixes on
   wago.tools for a few days (the script then stops), while its items still come only from
   them; the spell and talent hotfixes usually reach the next client build itself. Until then
   the item hotfixes of the last build that has them do: `python tools/fetch_tables.py
   --hotfix-build 1.60.1.70235 --hotfix-tables 'Item*'` (`fetch_hotfixes.py --from-build …
   --tables …`); the other tables stay raw. The current tables are build 70291 with 70235's
   item hotfixes (every other 70235 hotfix row was already in the 70291 tables).
   And `python tools/fetch_listfile.py`
   refreshes `data/tables/listfile-icons.csv`. That file holds the `interface/icons/` rows of
   the community listfile (https://github.com/wowdev/wow-listfile): the texture name of each
   icon FileDataID, by which Wowhead's CDN serves the icon. The client tables do not have
   these names. The script ends by counting the exported `icon:` ids that have a name.
   `csim-tables` loads the file with the tables (or the one `--listfile` names). The spell and
   item exports then write `icon_name: inv_sword_39` after each `icon:`. Without the file they
   warn and write no names, and they warn about icons the listfile does not name.
2. Run, from the repository root:
   ```
   cargo run -p csim-tables -- export-all
   ```
   which runs every export below in this order, then `check` (`--strict` passes through):
   ```
   cargo run -p csim-tables -- export-spells --class warrior
   cargo run -p csim-tables -- export-spells --class paladin
   cargo run -p csim-tables -- export-spells --class rogue
   cargo run -p csim-tables -- export-spells --racials
   cargo run -p csim-tables -- export-spells --externals
   cargo run -p csim-tables -- export-spells --enchants
   cargo run -p csim-tables -- export-talents --class warrior
   cargo run -p csim-tables -- export-talents --class paladin
   cargo run -p csim-tables -- export-talents --class rogue
   cargo run -p csim-tables -- export-items
   cargo run -p csim-tables -- export-spells --items
   cargo run -p csim-tables -- check
   ```
   `export-spells --items` walks the spells the items and set bonuses grant into
   `data/spells/items.yaml`; run it after the other spell exports, since spells another file
   already carries are not repeated. A weapon's chance-on-hit spell only runs with a rate in
   `overrides/items.yaml` (`proc: { chance: … }` or `{ ppm: … }`): the tables do not have it. `export-items` also prints why items were skipped and what it could not resolve.
   The exporter prints what it pruned and warns when an override mentions a spell that no
   longer exists; `check` lists the effects that need a script (or `IGNORED`) in the overrides
   and fails with `--strict` if there are any. `export-talents` warns when a node's `TraitCond`
   gate is not the `points_per_tier × tier` rule or a node has no tab or spell.
3. Look at the diff of `data/spells/*.yaml`, `data/talents/*.yaml` and `data/items/*.yaml`: new
   ranks, changed numbers, new payloads, moved talents, re-tuned items.
   `cargo run -p csim-tables -- spell <id>` prints one spell straight from the tables,
   `cargo run -p csim-tables -- item <id>` one item (stats, spells, set, suffix pools).
4. `cargo test` — `crates/csim-tables/tests/shipped.rs` checks the committed spell, talent and
   item files match a fresh export when `data/tables/` is present, and that the items differ
   from their hand-authored Classic version only as reviewed in
   `crates/csim-tables/tests/fixtures/classic_item_differences.txt` (regenerate with
   `csim-tables compare-items` after reviewing the change). The parity tests in
   `crates/csim-engine/src/spell/runtime/parity.rs` run the worked examples of §1.8.
5. Regenerate the test fixtures if the spells they use changed:
   `python crates/csim-tables/tests/fixtures/make_fixtures.py`.

A different build changes the `build:` header; every file in `data/spells/` must carry the
same build.

## External buffs (`data/external_buffs.yaml`)

The buffs other players and consumables provide are not hand-written numbers either: each
entry of `external_buffs.yaml` names the *aura* spell that ends up on the player or the target
(the buff, not the totem / item / cast that puts it there — Strength of Earth 25362, not the
totem spell 25361; Well Fed 24799, not the Smoked Desert Dumplings food cast), and the engine
builds the buff from that record like any other. The registry only adds what the tables do not
have: `faction` (ALLIANCE / HORDE, absent = both), `classes` the buff is offered to (absent =
all), a `mutex` key for the groups that exclude each other (one food, one strength elixir, …)
and `stacks` for a stacking debuff kept up by others (absent = the spell's `max_stacks`:
Sunder Armor ×5, Armor Shatter ×3). Selected buffs are applied once and stay applied across
iterations; the numbers change by re-exporting `externals.yaml`, not by editing the registry.
World buffs are deliberately absent (not available in Forever the same way).

Its `consumables` are items used in combat from the bags (Thistle Tea), named by item id: the
export writes the item's use effects (spell, item cooldown, shared category cooldown, from
`ItemEffect`) into `externals.yaml` as `consumable_items` next to the spells they cast. A
character's `consumables` list grants them like a trinket's use; the rotation casts one by the
consumable's name.

## What goes in the overrides

The tables describe *what* a spell does; a few things they do not carry are written by hand in
`data/spells/overrides/<file>.yaml` (schema: `crates/csim-engine/src/spell/overrides.rs`,
reference: `SPELL_INSTRUCTIONS.md` §1.11). One entry per spell id, `note` says why:

- **`effects`** — a script for a `DUMMY` effect or aura: `EXECUTE`, `ATTACK_POWER_PERCENT_DAMAGE`,
  `DEEP_WOUNDS_BLEED`, `TRIGGER_WITH_VALUE`, `PERIODIC_RESOURCE_GAIN`, … or `NO_OP` for a dummy
  that does nothing in the simulator. `csim-tables check` says which effects still need one.
- **`proc`** — the hit results a proc fires on (`hit_mask: [CRITICAL]` for Flurry, Deep Wounds):
  retail keeps this in the server-side `spell_proc` table; and `chance_effect` for the talents
  whose rank value is the proc chance (Unbridled Wrath 12–60 %) rather than the payload's value.
- **`threat`** — innate threat (`flat`) and a multiplier; the client has no threat table.
- **`sim_flags`** — how the simulator treats the spell: `IGNORED` (loads, never cast),
  `RESETS_SWING_TIMERS` / `STOPS_ATTACK_DURING_CAST` / `CANCELS_NEXT_SWING_QUEUE` (Slam),
  `RUN_TO_TARGET` (Charge: its cast time is the run, during which only offensive and cast-time
  spells wait),
  `START_OF_COMBAT` (Anger Management), `CANNOT_CRIT`, `ENRAGE` (Enrage: the `ENRAGED` aura state).
- **`stance_passive`** — the hidden passive carrying a stance's numbers
  (`SpellShapeshiftForm.PresetSpellID` is empty in the dump).
- **`on_event`** — a reaction to a combat event (Overpower's combo point on a dodge).
- **`ends_auras`** — the spells whose buffs end with this spell's buff (Jom Gabbar's attack power
  stacks, which have no duration in the tables).
- **`debuff_priority`**, **`debuff_shared`** — per-spell values where the file `defaults` do
  not fit.

What does *not* go there: numbers the tables have (costs, cooldowns, damage, durations, masks),
which spells a talent modifies (`EffectSpellClassMask`), ranks (`SupercedesSpell`), proc
sources (`ProcTypeMask`). If a number looks wrong, check the dump before overriding it.

## Pruning

`data/spells/overrides/discard.txt` lists the aura types and effect kinds a damage simulator
has no use for (crowd control, movement, immunities, healing, …). They are **not part of the
engine's vocabulary**: `crates/csim-engine/src/spell/dbc/aura.rs` and `effect.rs` do not name
them (they load as `UNKNOWN_<id>`), and `dbc/discard.rs` lists their ids so the exporter
(`crates/csim-tables/src/export/prune.rs`) can drop them: those effects go, spells with
nothing left go, triggers of dropped spells go, to a fixed point. Spells the overrides mention
are always kept. Details and the exceptions in `SPELL_INSTRUCTIONS.md` §1.10. Changing the
list means removing (or re-adding) the variant in `aura.rs` / `effect.rs`, updating the ids
in `discard.rs` and re-exporting.
