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

The tables are the client build plus its server-side hotfixes (talent reworks, spell values and
whole items come as hotfixes; Wowhead shows them). `python tools/fetch_tables.py` applies them;
when Wowhead differs from the data on the same build, refresh them with
`python tools/fetch_hotfixes.py --force` and re-export (`data/README.md`).

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

**NEVER PUSH ANYTHING TO THE REMOTE REPOSITORY.**

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
that returned true; a Breakdown pane below the stage shows the damage so far per spell, the buff uptimes, the procs (count, proc rate,
PPM) and the resource gained per source, as `csim run` breaks them down). It takes the setup, `--seed`, `--length`,
`--length-variance` and `--port`, one character only (no `--raid`):

`cargo run --release -p csim-live -- data/characters/warrior_fury_dw_orc.yaml --seed 1`

The flags only load the first iteration. A setup bar on the page loads another: a bundled setup
(`data/characters/`), played by its rotation or with bundled keybinds (`data/keybinds/`), with a
seed, length, variance and named settings. Choosing a setup or how it is played loads it right
away, with the bar's seed; keybinds that do not fit the new setup fall back to its rotation. The
named settings get a control each, by their kind in `named_settings.rs` (a choice a dropdown,
which loads right away; a number a number field; a setting that requires another's value shows
only with it). A separate Target dropdown overrides the setup's target creature type (loads right
away) and armor. The text fields and pasted YAML wait for Load. A setup or keybinds can also be pasted or uploaded as
YAML; a pasted setup's `include:` lines name the bundled files. The address mirrors what is
loaded (`?setup=warrior_fury_dw_orc&keybinds=dw_fury&seed=1&setting=...`), so a link reproduces
the iteration. "Edit keybinds" opens an editor: click a spell's key, then press the new one
(Esc cancels, Backspace or Delete unbinds, a key in use moves), and build macros. It starts from
the loaded keybinds, the class's last applied ones (browser storage) or a bundled file. Apply
loads them as pasted keybinds; a link carries pasted or edited keybinds as `keys=` (their YAML,
base64url). "Equipment" opens the gear: a paper doll with the stat summary (melee, ranged,
spell; the setup's stats before the iteration, without the precombat casts) and the items the
character can wear in the selected slot (`api/items`), sortable by name, item level, quality,
type, and DPS and speed in the weapon slots, with a name search and tooltips. Choosing an item,
or taking one off (right click, ×), loads the setup again with the changed gear (equipping as
the game does: a two-hander empties the off hand; a new item keeps the slot's enchants that
fit it); Reset goes back to the setup's own. The setup bar's loads keep the changes until
another setup is chosen, and a link carries them as `gear=mh-19019.oh-0` (slot code, item id,
`0` empties the slot). "Talents" opens the talent calculator: the class's three trees as the
game draws them (the tab art from Wowhead, prerequisite arrows, ranks lit when a point can go
in), with each rank's text in a tooltip (`descriptions` of `data/talents/`, resolved at export).
Click learns a rank, right click (or a long press) unlearns one, Shift does all of them, × clears
a tree; the rules are the engine's (`api/talents/edit`). The edits are a draft: Apply loads the
setup with them (an error, e.g. a rotation's missing prerequisite, shows and the draft stays),
Reset goes back to the setup's own. Like gear, the changed talents stay over the setup bar's loads
until another setup is chosen, and a link carries them as `talents=05-0505311515201` (Wowhead's
classic string: a digit per talent in tier then column order, a part per tree). "Buffs & Debuffs"
opens the external buffs and debuffs the character is offered (`data/external_buffs.yaml`, by
class and faction), a column each: a row per entry with its icon and name, lit when the session
has it. Clicking one loads the setup again with it added or removed (adding drops the entries of
its `mutex`); Reset goes back to the setup's own. Like gear, the changed lists stay over the
setup bar's loads until another setup is chosen, and a link carries them as
`buffs=Juju Power,Grilled Squid` / `debuffs=` (names; empty for none). Without a setup
argument the page picks one:

`cargo run --release -p csim-live`

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

Its spell and item icons come from Wowhead's CDN, by the `icon_name` of the exported spells and
items (texture names from the community listfile, see `tools/fetch_listfile.py`). An icon
without a name, or one that does not load (offline), falls back to `data/icons/`: a gitignored
local cache of the game's textures, by their `icon` FileDataIDs. Fill it once, and again after
a re-export, when needed:

`python tools/fetch_icons.py`

The same page also runs without a server, at https://timhul.github.io/ClassicSimForever/live/:
the `csim-web` crate compiles the server side (and the data it needs) to WebAssembly, and the
page calls it in place of HTTP. `.github/workflows/pages.yml` builds and deploys it on each push
to `main`. The site has no `data/icons/` fallback. Build it locally into `site/` (gitignored;
needs the `wasm32-unknown-unknown` target and the `wasm-bindgen-cli` version that Cargo.toml
pins, `wasm-opt` optional):

`python tools/build_web.py`

`python -m http.server -d site 8000`

then open http://localhost:8000/ (it redirects to `/live/`). That the wasm build still compiles
(what the workflow checks on pull requests):

`cargo check -p csim-web --target wasm32-unknown-unknown`

Named settings switch to alternatives that are not the default behavior: `--setting` takes
comma-separated `name:value` pairs and may be repeated.

`cargo run --release -p csim-cli -- run data/characters/warrior_fury_dw_orc.yaml --setting=rage_formula:marrow_sigmoid,sigmoid_floor:0,sigmoid_ceiling:46,sigmoid_midpoint:58,sigmoid_width:3.8`

`initial_rage:N` starts every iteration with N rage (before the precombat actions; ignored by a
character without rage):

`cargo run --release -p csim-cli -- run data/characters/warrior_fury_dw_orc.yaml --setting=initial_rage:50`

`target_start_health_percent:N` (1-100, default 100) starts the target at N % health; it falls
linearly to 0 at the end, which places every execute range (Execute below 20 %, Quietus below
35 %, ...). At 30 Execute is usable for the last two thirds and Quietus for the whole fight. It
only sets the health (no Essence of the Red, unlike the Vaelastrasz ruleset, which imposes 30):

`cargo run --release -p csim-cli -- run data/characters/rogue_combat_swords_human.yaml --setting=target_start_health_percent:30`

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

- The hotfixes are scraped from the wago.tools/hotfixes web page (no API). If the page changes,
`tools/fetch_hotfixes.py` stops with an error and leaves the tables as they were.

- The browser viewer hotlinks its icons from Wowhead's CDN, which is not ours to guarantee: if it
breaks, the site shows empty icon slots. Forever-only textures have no name in the listfile, so
they show no icon on the site either.

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
