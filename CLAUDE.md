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

The combat log (`WoWCombatLog.txt` lines) of one iteration, the same one `-n 1 -t 1` simulates with
that seed:

`cargo run --release -p csim-cli -- run data/characters/dw_fury_orc.yaml --combat-log --seed 1`

In a raid (`data/raids/`, members refer to `data/characters/`):

`cargo run --release -p csim-cli -- run data/characters/dw_fury_orc.yaml --raid data/raids/horde_melee.yaml`

Stat weights per item stat point, then items ranked by them (static stats only; weapon damage,
effects, set bonuses and suffixes are not scored):

`cargo run --release -p csim-cli -- run data/characters/dw_fury_orc.yaml --scale --weights-file weights.yaml`

`cargo run --release -p csim-cli -- rank-items --weights weights.yaml --slot gloves`

`cargo run --release -p csim-cli -- rank-items --weights weights.yaml --slot mainhand --type axe,sword,mace,dagger,fist`

Sweeps (`data/sweeps/`, schema in `crates/csim-engine/src/sweep_loader.rs`): a base character setup
plus variation points (`talent_points`: every way to spend exactly N more points over some talents;
`options`: explicit overrides of race, rotation, equipment slots, ...). Every combination is simmed
with the same seed and ranked by DPS; `--dry-run` only counts and lists the variants:

`cargo run --release -p csim-cli -- sweep data/sweeps/dw_fury_last_3_points.yaml --dry-run`

# Known issues

- Raid DPS is much lower than solo DPS. This is because debuffs are not applied in the current
raid setup, and many raid buffs are missing.
