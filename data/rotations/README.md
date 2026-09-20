# `data/rotations/` — what a character casts and when

One YAML file per rotation, in a subdirectory named after the class (`warrior/`, `rogue/`, …).
The engine scans every class subdirectory (`RotationDb::load`, schema in
`crates/csim-engine/src/rotation/spec.rs`); the C++ `rotation_paths.xml` is gone. A file's
`class` must match its directory, and `(class, name)` must be unique.

## Schema

```yaml
class: WARRIOR                # the class (SCREAMING_SNAKE_CASE, as in data/classes/)
name: DW Fury High Rage       # display name, unique within the class
attack_mode: melee            # melee | ranged | magic; default melee
description: >-               # free text; whitespace is collapsed on load
  A rotation for dual-wield fury that does not attempt to dump rage before
  switching to Battle Stance for Overpower.
precombat_actions:            # cast before the pull, in this order; default none
  - Bloodrage
  - Battle Shout
  - Berserker Stance
precast: Aimed Shot           # optional: a cast started early so it completes at t = 0
cast_if:                      # the executors, highest priority first; default none
  - name: Bloodrage
    condition: resource "Rage" less 70
  - name: Battle Shout
    condition: |
      buff_duration "Battle Shout" less 3
      or variable "time_remaining_execute" less 10
      and variable "time_remaining_execute" greater 0
      and buff_duration "Battle Shout" less 45
  - name: Overpower           # no condition: cast whenever the spell is available
  - name: Heroic Strike
    rank: 8                   # optional; absent = the highest learned rank
    condition: resource "Rage" greater 65
```

- `name` in `cast_if` is the spell name a character knows it by (its rank group name). An
  executor whose spell the character does not have (a trinket not equipped, a racial of
  another race, a talent not taken) is skipped when the rotation is linked.
- The same spell may appear in several executors with different conditions (Arms has two
  `Heroic Strike` lines); they are tried in order.
- `rank` may not be `0`: leave it out to cast the highest rank.
- A `condition` that is present must not be blank; leave it out to mean "whenever available".
  This keeps a typo from silently turning into an unconditional cast.

## Conditions

One sentence per line. The first line is a sentence; every following line starts with `and`
or `or`. `or` splits the condition into groups; the sentences of a group are AND-ed, and the
executor fires when any group holds (the C++ `RotationExecutor` semantics: no parentheses,
no precedence beyond that).

```
<type> "<value>" <comparison>
```

| type            | value                          | measures                                  |
|-----------------|--------------------------------|-------------------------------------------|
| `buff_duration` | a buff name                    | remaining duration in seconds (0 if down) |
| `buff_stacks`   | a buff name                    | current stacks                            |
| `spell`         | a spell name                   | cooldown remaining in seconds             |
| `resource`      | `Rage` / `Mana` / `Energy` / `Focus` | current amount                       |
| `variable`      | a builtin (below)              | its value                                 |

Comparisons: `less`, `leq`, `eq`, `geq`, `greater` followed by a number, or `is true` /
`is false` (for `buff_duration`: whether the buff is up).

Builtin variables: `target_health`, `time_remaining_encounter`, `time_remaining_execute`,
`swing_timer`, `melee_ap`, `combo_points`, `time_remaining_gcd`.

The grammar is parsed by `crates/csim-engine/src/rotation/condition.rs` (Phase 5.2); until
then the text is stored as written and only checked for being non-blank.
