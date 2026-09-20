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

Comparisons: `less`, `leq`, `eq`, `geq`, `greater` followed by a number (`leq` / `geq` /
`eq` are within 0.0001), or `is true` / `is false` — only for the buff types: whether the
buff is up (`buff_duration`) or has any stacks (`buff_stacks`).

Builtin variables:

| variable                   | value                                                        |
|----------------------------|--------------------------------------------------------------|
| `target_health`            | remaining target health as a fraction of the fight, 1 → 0    |
| `time_remaining_encounter` | seconds until the fight ends                                 |
| `time_remaining_execute`   | seconds until the execute phase; negative once in it         |
| `time_since_swing`         | seconds since the last main hand swing                       |
| `time_since_auto_shot`     | seconds since the last auto shot                             |
| `melee_ap`                 | melee attack power                                           |
| `combo_points`             | combo points on the target                                   |
| `time_remaining_gcd`       | seconds until the global cooldown ends                       |

Every condition is parsed when the file is loaded (`crates/csim-engine/src/rotation/condition.rs`);
a line that does not follow the grammar, an unknown type, resource or variable, or `is` on a
non-buff type is a load error naming the file, the executor and the line. Buff and spell names
are only resolved when the rotation is linked to a character; an executor naming something the
character does not have is skipped then, not rejected here.

## Linking and running

A rotation is linked to a character (`Rotation::link`, again before every set of iterations):
an executor is *active* when the character has learned its spell at the asked rank (the
highest learned rank without `rank`), the spell is enabled (a talent taken, the race's racial,
the item equipped) and every buff and spell its condition names resolves. The rest are skipped
and never attempted. `spell "<name>"` in a condition resolves to the highest learned rank
whether or not it is enabled (a disabled Bloodthirst has a cooldown of 0).

Before the pull (at negative time) the precombat actions are cast in order when their spell is
available or merely on cooldown (every cooldown reads as "ready at 0" then), followed by the
precast. The rotation itself never runs before the pull; the first pass is the encounter start,
after which every `PlayerAction` (a global cooldown or a cooldown ending, a rage gain, the
stance swap lag, a completed cast) runs the active executors in order: a spell that is not
available counts its status, one whose condition does not hold counts a failed condition, and
one that is cast counts a successful cast. Those counts are the executor statistics.
