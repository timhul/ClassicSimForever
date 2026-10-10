# Introduction

This project contains ClassicSimForever, a simulator for World of Warcraft: Forever.

# Main technical differences

ClassicSimForever uses Rust instead of C++.

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

Code comments must be short. Do not describe *what* changed, that is explained by the code.

Commit each defined task once complete. Commit messages must be very short. Do *not* describe
what changed, that is already shown by the code changes.

**NEVER PUSH ANYTHING TO THE REMOTE REPOSITORY.**

## Running the sim

Single character, no raid.

`cargo run --release -p csim-cli -- run data/characters/warrior_fury_dw_orc.yaml --iterations 10000`

Fixed seed, single character.

`cargo run --release -p csim-live -- data/characters/warrior_fury_dw_orc.yaml --seed 1`

Building and viewing `csim-live` locally:

`python tools/build_web.py`

`python -m http.server -d site 8000`

then open http://localhost:8000/ (it redirects to `/live/`). That the wasm build still compiles
(what the workflow checks on pull requests):

Check that `wasm32` for `csim-live` works.

`cargo check -p csim-web --target wasm32-unknown-unknown`

Named settings.

`cargo run --release -p csim-cli -- run data/characters/warrior_fury_dw_orc.yaml --setting=initial_rage:50`

`cargo run --release -p csim-cli -- run data/characters/rogue_combat_swords_human.yaml --setting=target_start_health_percent:30`

Rogue.

`cargo run --release -p csim-cli -- run data/characters/rogue_combat_swords_human.yaml --iterations 10000`

Paladin.

`cargo run --release -p csim-cli -- run data/characters/paladin_ret_2h_human.yaml --iterations 10000`

Raid sim. Raid DPS can be lower than solo DPS depending on provided party buffs and debuffs.

`cargo run --release -p csim-cli -- run data/characters/warrior_fury_dw_orc.yaml --raid data/raids/horde_melee.yaml`

Sweeping multiple sets of iterations for easier comparisons between variation points (talents,
equipments, etc.).

`cargo run --release -p csim-cli -- sweep data/sweeps/rogue_profiles.yaml`

`cargo run --release -p csim-cli -- sweep data/sweeps/warrior_profiles.yaml`

`cargo run --release -p csim-cli -- sweep data/sweeps/ret_profiles.yaml`


# Good cross-reference information sources

https://github.com/ClassicWoWCommunity/forever-bugs/issues/
https://github.com/magey/forever-warrior
https://ppach-warriorcompendium.share.connect.posit.cloud/

Treat github comments by user "AidanZMoon" and "Magey" as ground truth.

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
