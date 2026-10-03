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

`cargo run --release -p csim-cli -- run data/characters/warrior_fury_dw_orc.yaml --iterations 10000`

The Forever race Skyborne is two races, one per faction: `HIGH_ORDER_SKYBORNE` (Alliance) and
`WINDSHAPER_SKYBORNE` (Horde). Its DW Fury profile is the Horde one:

`cargo run --release -p csim-cli -- run data/characters/warrior_fury_dw_skyborne.yaml --iterations 10000`

The combat log (`WoWCombatLog.txt` lines) of one iteration, the same one `-n 1 -t 1` simulates with
that seed:

`cargo run --release -p csim-cli -- run data/characters/warrior_fury_dw_orc.yaml --combat-log --seed 1`

The same iteration, watched live in a browser (`csim-live` serves a page on
http://127.0.0.1:7878 with play / pause, speed, step event / step to cast and restart; the engine
is used as a library, as by the CLI; a Rotation pane lists each cast with the `cast_if` entry
that returned true; a Breakdown pane below the stage shows the damage so far per spell, the buff uptimes and the procs (count, proc rate,
PPM), as `csim run` breaks them down). It takes the setup, `--seed`, `--length`,
`--length-variance` and `--port`, one character only (no `--raid`):

`cargo run --release -p csim-live -- data/characters/warrior_fury_dw_orc.yaml --seed 1`

With `--keybinds` the rotation does not run: the character is played from the keyboard. The file
maps spell names to keys (`'Bloodthirst': 1`, `Execute: Shift+E`, `Recklessness: Ctrl+Alt+F1`;
Ctrl, Shift and Alt as modifiers); a spell without a key cannot be cast, and a press while the
spell is not castable yet waits 0.4 s for it (the game's spell queue window). A key can also
cast a macro (`Name: {hotkey: T, cast: [Spell1, Spell2]}`), tried in order like the game's `/cast`
lines: it stops after a spell that triggers the GCD. The player pulls: the iteration starts 10 min
before the encounter, and the first offensive spell (one hitting the target: Charge, Bloodthirst;
not Bloodrage, Battle Shout or a stance) starts it when that spell lands (Charge's 1 s run). The
page then shows every time again with the pull at 0. Examples are in `data/keybinds/`:

`cargo run --release -p csim-live -- data/characters/warrior_fury_dw_orc.yaml --keybinds data/keybinds/dw_fury.yaml`

Its spell and item icons come from `data/icons/` (a gitignored cache of the game's textures, by
the `icon` FileDataIDs of the exported spells and items). Fetch them once, and again after a
re-export; without them the page shows no icons:

`python tools/fetch_icons.py`

A Rogue is run the same way, with a rogue setup (`rogue_combat_swords_human`, `rogue_combat_axes_orc`,
`rogue_combat_daggers_night_elf`, `rogue_mutilate_undead`, `rogue_mutilate_ea_gnome`, `rogue_hemorrhage_troll`; their
rotations are in `data/rotations/rogue/`):

`cargo run --release -p csim-cli -- run data/characters/rogue_combat_swords_human.yaml --iterations 10000`

In a raid (`data/raids/`, members refer to `data/characters/`):

`cargo run --release -p csim-cli -- run data/characters/warrior_fury_dw_orc.yaml --raid data/raids/horde_melee.yaml`

`cargo run --release -p csim-cli -- run data/characters/rogue_combat_axes_orc.yaml --raid data/raids/horde_rogues.yaml`

Stat weights per item stat point, then items ranked by them (static stats only; weapon damage,
effects, set bonuses and suffixes are not scored):

`cargo run --release -p csim-cli -- run data/characters/warrior_fury_dw_orc.yaml --scale --weights-file weights.yaml`

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

A rotation can name the spells it cannot do without (`prerequisite: Mortal Strike`, one or a
list): a setup whose character lacks one (no such spell, or its talent not taken) is invalid, so
`run` rejects it and a sweep skips that variant (e.g. the arms rotation on a fury character).

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

# Class colors

If class colors are referenced:

Druid   255 124 10  1.00    0.49    0.04    #FF7C0A
Hunter  170 211 114 0.67    0.83    0.45    #AAD372
Mage    63  199 235 0.25    0.78    0.92    #3FC7EB
Paladin 244 140 186 0.96    0.55    0.73    #F48CBA
Priest  255 255 255 1.00    1.00    1.00    #FFFFFF
Rogue   255 244 104 1.00    0.96    0.41    #FFF468
Shaman  0   112 221 0.00    0.44    0.87    #0070DD
Warlock 135 136 238 0.53    0.53    0.93    #8788EE
Warrior 198 155 109 0.78    0.61    0.43    #C69B6D
