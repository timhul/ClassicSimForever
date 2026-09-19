# `data/` — what the simulator reads

Everything the engine knows about the game is data: exported from the client table dumps where
the tables have it, hand-written where they do not. Nothing about a specific spell is hardcoded
in Rust; adding or updating spells means re-exporting from a new dump and, occasionally, adding
an override.

```
data/
├── tables/               DB2 → CSV dumps, one file per table and build (gitignored, see below)
├── spells/
│   ├── warrior.yaml      generated: the Warrior spellbook, talents, runes and their payloads
│   ├── racials.yaml      generated: the racial abilities of every race
│   └── overrides/
│       ├── warrior.yaml  hand-written: what the tables do not say (scripts, threat, sim flags)
│       ├── racials.yaml
│       └── discard.txt   the effects the exporter drops (see "Pruning")
├── items/ enchants.yaml set_bonuses.yaml   Phase 2 item data (see ITEM_INSTRUCTIONS.md)
├── races.yaml            hand-written: ids/factions from ChrRaces, base attributes (racials are spells)
├── classes/<class>.yaml  hand-written: stat rules from ChrClasses / PlayerExpectedStat, races from
│                         CharBaseInfo, base stats, proficiencies, enchant lists per slot
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
csim-tables export-spells --racials         →  data/spells/racials.yaml
        │  + data/spells/overrides/*.yaml
        ▼
csim_engine::spell::record::SpellDb::load("data/spells")   (the engine)
```

1. **`csim-tables`** reads the CSV dumps (`TableDir` finds the build from the file names,
   `Tables` indexes the rows) and walks a class: its skill lines (`SkillLineAbility`), its
   talent tree (`Trait*`), the payload spells those reach through `EffectTriggerSpell`,
   `OVERRIDE_ACTIONBAR_SPELLS`, `SpellAuraRestrictions` and the overrides' references
   (`SPELL_INSTRUCTIONS.md` §1.3–1.6). Each spell becomes one `SpellRecord`: the joined
   `Spell*` rows in snake_case (§1.7), effects included.
2. The walk is **pruned** (§1.10): effects the simulator has no use for are dropped, spells left
   with nothing to do are left out.
3. The generated files are committed so the engine and its tests never need the dump.
4. The engine loads the generated files plus the **overrides** and checks the cross references
   (`SpellDb::load`). `Spell`, `Buff`, `Effect`, `Proc` and `Periodic` are built from the
   records at runtime (`crates/csim-engine/src/spell/`).

## Re-exporting from a new dump

1. Drop the CSV files in `data/tables/` (`<Table>.<build>.csv`; `csim-tables info` lists what
   is there and what is missing).
2. Run, from the repository root:
   ```
   cargo run -p csim-tables -- export-spells --class warrior
   cargo run -p csim-tables -- export-spells --racials
   cargo run -p csim-tables -- check
   ```
   The exporter prints what it pruned and warns when an override mentions a spell that no
   longer exists; `check` lists the effects that need a script (or `IGNORED`) in the overrides
   and fails with `--strict` if there are any.
3. Look at the diff of `data/spells/*.yaml`: new ranks, changed numbers, new payloads.
   `cargo run -p csim-tables -- spell <id>` prints one spell straight from the tables.
4. `cargo test` — `crates/csim-tables/tests/shipped.rs` checks the committed files match a
   fresh export when `data/tables/` is present, and the parity tests in
   `crates/csim-engine/src/spell/runtime/parity.rs` run the worked examples of §1.8.
5. Regenerate the test fixtures if the spells they use changed:
   `python crates/csim-tables/tests/fixtures/make_fixtures.py`.

A different build changes the `build:` header; every file in `data/spells/` must carry the
same build.

## What goes in the overrides

The tables describe *what* a spell does; a few things they do not carry are written by hand in
`data/spells/overrides/<file>.yaml` (schema: `crates/csim-engine/src/spell/overrides.rs`,
reference: `SPELL_INSTRUCTIONS.md` §1.11). One entry per spell id, `note` says why:

- **`effects`** — a script for a `DUMMY` effect or aura: `EXECUTE`, `ATTACK_POWER_PERCENT_DAMAGE`,
  `DEEP_WOUNDS_BLEED`, `TRIGGER_WITH_VALUE`, `PERIODIC_RESOURCE_GAIN`, … or `NO_OP` for a dummy
  that does nothing in the simulator. `csim-tables check` says which effects still need one.
- **`proc`** — the hit results a proc fires on (`hit_mask: [CRITICAL]` for Flurry, Deep Wounds):
  retail keeps this in the server-side `spell_proc` table.
- **`threat`** — innate threat (`flat`) and a multiplier; the client has no threat table.
- **`sim_flags`** — how the simulator treats the spell: `IGNORED` (loads, never cast),
  `RESETS_SWING_TIMERS` / `STOPS_ATTACK_DURING_CAST` / `CANCELS_NEXT_SWING_QUEUE` (Slam),
  `START_OF_COMBAT` (Anger Management), `CANNOT_CRIT`, `ENRAGE` (Enrage: the `ENRAGED` aura state).
- **`stance_passive`** — the hidden passive carrying a stance's numbers
  (`SpellShapeshiftForm.PresetSpellID` is empty in the dump).
- **`on_event`** — a reaction to a combat event (Overpower's combo point on a dodge).
- **`resource_miss_cost_mod`**, **`debuff_priority`**, **`debuff_shared`** — per-spell values
  where the file `defaults` do not fit.

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
