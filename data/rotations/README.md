# `data/rotations/` — what a character casts and when

One YAML file per rotation, in a subdirectory named after the class (`warrior/`, `rogue/`,
`paladin/`).

| class | file | name | prerequisite |
|---|---|---|---|
| Warrior | `dw_fury.yaml` | DW Fury | Bloodthirst |
| Warrior | `arms.yaml` | Mortal Strike | Mortal Strike |
| Warrior | `prot.yaml` | Protection | — |
| Rogue | `combat.yaml` | Combat | — |
| Rogue | `combat_dagger.yaml` | Combat Dagger | — |
| Rogue | `hemorrhage.yaml` | Hemorrhage | — |
| Rogue | `seal_fate_mutilate.yaml` | Seal Fate Mutilate | — |
| Rogue | `seal_fate_ea.yaml` | Seal Fate Mutilate Expose Armor | — |
| Paladin | `seal_twisting.yaml` | Seal Twisting (the Ret setups' default) | Twist of Light |
| Paladin | `seal_of_command.yaml` | Seal of Command | Seal of Command |
| Paladin | `seal_of_the_crusader.yaml` | Seal of the Crusader | Seal of Command |

The Paladin's rotations, for Forever's seals: Judgement leaves the seal up, so the seal is
judged on cooldown; Judgement of the Crusader (+161 holy damage taken) is refreshed by every
melee strike and judged again only when it runs low; Seal Twisting casts Seal of Righteousness
right before a swing while Seal of Command is up, so that the swing deals both (Twist of
Light's Echo of Command), and puts the other global cooldowns where they end before the next
twist. Holy Strike goes on cooldown, Hammer of Wrath in the execute range, Exorcism against
Undead and Demons only, Consecration while the mana allows.

## Schema

```yaml
class: WARRIOR
name: DW Fury                 # display name, must be unique within the class
attack_mode: melee            # melee | ranged | magic
description: >-               # descriptive only
  A rotation for dual-wield fury.
precombat_actions:            # cast before the pull in the specified order
  - Bloodrage
  - Battle Shout
  - Berserker Stance
precast: Aimed Shot           # optional: a cast (with timer) started its cast time before t = 0,
                              #   after the precombat actions, so that it completes at t = 0
prerequisite: Bloodthirst     # optional, one or a list: the spells (or talents without a spell,
                              #   Twist of Light) a character needs for the rotation; a setup
                              #   lacking one is rejected by `run`, skipped by a sweep
cast_if:                      # cast_if's are evaluated in order with no early return
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
    rank: 8                   # optional: absent = the highest learned rank
    condition: resource "Rage" greater 65
```

- if `name` in `cast_if` can't be resolved (e.g. Blood Fury but simming a Troll) it is dropped
  from the executor queue before combat starts.
- The same spell may appear in several executors with different conditions.

## Conditions

One sentence per line. The first line is a sentence; every following line starts with `and`
or `or`. `or` splits the condition into groups; the sentences of a group are AND-ed, and the
executor fires when any group holds.

```
condition:
    condition_A
    and condition_B
    or condition_C
```

is equivalent to `(A and B) or (C)`.


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
| `talent`        | a talent name                  | points spent in it                        |

Comparisons: `less`, `leq`, `eq`, `geq`, `greater` followed by a number (`leq` / `geq` /
`eq` are within 0.0001), or `is true` / `is false` — only for the buff types: whether the
buff is up (`buff_duration`) or has any stacks (`buff_stacks`).

The buff types read the character's own debuffs on the target too, by the debuff's name:
`buff_duration "Rupture" less 2` or `buff_duration "Expose Armor" less 3` time a Rogue's
refresh, `buff_stacks "Sunder Armor" less 5` a Warrior's.

An on-next-swing spell (Heroic Strike, Cleave) is a buff of its own name while it is queued,
from the cast until the main hand swing that takes it (or drops it, without the rage for it).
Casting it again while queued changes nothing but the executor's cast count, so a rotation can
skip it with `buff_duration "Heroic Strike" is false`. Queueing Cleave un-queues Heroic Strike
and the other way round.

`talent` is decided once, when the rotation is linked to the character, since talents do not
change during a fight: a sentence that holds drops out of its group, one that fails drops its
group. When no group is left the line is skipped ("condition needs Improved Berserker Rage
talent rank > 0"), as is a line naming a talent the class does not have. A line cast only
with the talent taken:

```
talent "Improved Berserker Rage" greater 0
and resource "Rage" less 50
```

`variable "target_is_type"` is compared by name, not number: `eq "<creature type>"` holds when
the target (`target: creature_type:` in the character setup) is of that type. The types are
Beast, Demon, Dragonkin, Elemental, Giant, Humanoid, Mechanical and Undead, in any case
(`eq "giant"`). Several types are `or`-ed groups:

```
variable "target_is_type" eq "giant"
or variable "target_is_type" eq "dragonkin"
```

Builtin variables:

| variable                   | value                                                        |
|----------------------------|--------------------------------------------------------------|
| `target_health`            | remaining target health as a fraction of the fight, 1 → 0    |
| `time_remaining_encounter` | seconds until the fight ends                                 |
| `time_remaining_execute`   | seconds until the execute phase; negative once in it         |
| `time_since_swing`         | seconds since the last main hand swing                       |
| `time_remaining_swing`     | seconds until the next main hand swing                       |
| `time_since_auto_shot`     | seconds since the last auto shot                             |
| `melee_ap`                 | melee attack power                                           |
| `combo_points`             | combo points on the target                                   |
| `time_remaining_gcd`       | seconds until the global cooldown ends                       |
| `resource_missing`         | what the resource lacks to its maximum (potion: `geq 2250`)  |
| `target_is_type`           | the target's creature type, compared with `eq "<type>"`      |

A character with energy or mana also acts when a timer condition flips by itself: an energy user
on the energy tick that sees it, a mana user right when it flips. A Paladin's seal twist before
the swing (Twist of Light's Echo):

```
- name: Seal of Righteousness
  condition: |-
    buff_duration "Seal of Command" is true
    and variable "time_remaining_swing" less 0.5
- name: Seal of Command
  condition: |-
    buff_duration "Seal of Command" is false
    and buff_duration "Echo of Command" is false
```
