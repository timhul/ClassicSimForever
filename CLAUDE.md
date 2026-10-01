# Introduction

This project contains ClassicSimForever, a simulator for World of Warcraft: Forever.
It is a C++ to Rust port from ClassicSim, a simulator for World of Warcraft: Classic.

# Main technical differences

ClassicSimForever uses Rust instead of C++ Qt. No GUI for now.

The data (spells, etc.) used to be authored by hand in the legacy C++ version. Now the data is
derived from the actual client side database tables. Not everything is present in these tables and
needs to be reimplemented.

There are also actual data differences due to the game versions (Forever vs Classic) being slightly
different.

# Development flow

(1) Planning phase

Create a plan for what to do, and write this plan to `TASKS.md`. If there is already
content in `TASKS.md` then append to the TASKS.md.

Do not implement anything during the planning phase.

(2) Development phase

Implement the given task in `TASKS.md`. Add tests if relevant.

Run `cargo clippy --all-targets -- -D warnings`, , `cargo fmt --check` `cargo nextest run` to verify
building works and source code passes linting.

Commit each defined task once complete.

Toolchain note: cargo/rustc are installed at `C:\Users\timhu\.cargo\bin` but are not on the shell
`PATH` in this environment; invoke via full path or add to PATH.

# Running the sim

`cargo run --release -p csim-cli -- run data/characters/dw_fury_orc.yaml --iterations 10000`

The Forever race Skyborne is two races, one per faction: `HIGH_ORDER_SKYBORNE` (Alliance) and
`WINDSHAPER_SKYBORNE` (Horde). Its DW Fury profile is the Horde one:

`cargo run --release -p csim-cli -- run data/characters/dw_fury_skyborne.yaml --iterations 10000`

The combat log (`WoWCombatLog.txt` lines) of one iteration, the same one `-n 1 -t 1` simulates with
that seed:

`cargo run --release -p csim-cli -- run data/characters/dw_fury_orc.yaml --combat-log --seed 1`

A Rogue is run the same way, with a rogue setup (`combat_swords_human`, `combat_axes_orc`,
`combat_daggers_night_elf`, `mutilate_undead`, `mutilate_ea_gnome`, `hemorrhage_troll`; their
rotations are in `data/rotations/rogue/`):

`cargo run --release -p csim-cli -- run data/characters/combat_swords_human.yaml --iterations 10000`

In a raid (`data/raids/`, members refer to `data/characters/`):

`cargo run --release -p csim-cli -- run data/characters/dw_fury_orc.yaml --raid data/raids/horde_melee.yaml`

`cargo run --release -p csim-cli -- run data/characters/combat_axes_orc.yaml --raid data/raids/horde_rogues.yaml`

Stat weights per item stat point, then items ranked by them (static stats only; weapon damage,
effects, set bonuses and suffixes are not scored):

`cargo run --release -p csim-cli -- run data/characters/dw_fury_orc.yaml --scale --weights-file weights.yaml`

`cargo run --release -p csim-cli -- rank-items --weights weights.yaml --slot gloves`

`cargo run --release -p csim-cli -- rank-items --weights weights.yaml --slot mainhand --type axe,sword,mace,dagger,fist`

`rank-items` only lists what the weights' class can use (armor type, weapon proficiencies per slot);
`list-items --class rogue` filters the same way.

Sweeps (`data/sweeps/`, schema in `crates/csim-engine/src/sweep_loader.rs`): a base character setup
plus variation points (`talent_points`: every way to spend exactly N more points over some talents;
`options`: explicit overrides of race, rotation, equipment slots, ...). Every combination is simmed
with the same seed and ranked by DPS; `--dry-run` only counts and lists the variants:

`cargo run --release -p csim-cli -- sweep data/sweeps/dw_fury_last_3_points.yaml --dry-run`

Instead of a base, a `characters` variation point lists whole character files, to sim profiles
against each other:

`cargo run --release -p csim-cli -- sweep data/sweeps/dw_fury_profiles.yaml`

The Rogue's: `data/sweeps/combat_swords_last_3_points.yaml` and `data/sweeps/dw_rogue_profiles.yaml`.

# Good cross-reference information sources

https://github.com/ClassicWoWCommunity/forever-bugs/issues/
https://github.com/magey/forever-warrior
https://ppach-warriorcompendium.share.connect.posit.cloud/

Treat github comments by user "AidanZMoon" and "Magey" as ground truth.

# Known issues

- Raid DPS can be lower than solo DPS. This is because external debuffs are not applied in a raid
context since they depend instead on the available raid members, meaning some debuffs are not
applied if characters applying those debuffs are.
