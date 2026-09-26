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

- `name` in `cast_if` is the spell name a character knows it by (its rank group name; an item's
  use goes by its spell's name: `Kiss of the Spider`, `Haste` for Manual Crowd Pummeler). An
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
| `target_is_type`           | the target's creature type, compared with `eq "<type>"`      |

Every condition is parsed when the file is loaded (`crates/csim-engine/src/rotation/condition.rs`);
a line that does not follow the grammar, an unknown type, resource or variable, or `is` on a
non-buff type is a load error naming the file, the executor and the line. Buff and spell names
are only resolved when the rotation is linked to a character (see below), not rejected here.

## Linking and running

A rotation is linked to a character (`Rotation::link`, again before every set of iterations):
an executor is *active* when the character has learned its spell at the asked rank (the
highest learned rank without `rank`), the spell is enabled (a talent taken, the race's racial,
the item equipped), every spell its condition names resolves and the condition can still hold.
A buff the character can never have (`Eureka!` for a non-Gnome) reads as down: a line that then
always holds (`is false`, `less 3`) is dropped from its group, one that never holds (`is true`)
drops its group, and an executor left with no group is skipped. The rest are skipped and never
attempted. `spell "<name>"` in a condition resolves to the highest learned rank
whether or not it is enabled (a disabled Bloodthirst has a cooldown of 0).

Before the pull (at negative time) the precombat actions are cast in order when their spell is
available or merely on cooldown (every cooldown reads as "ready at 0" then), followed by the
precast. The rotation itself never runs before the pull; the first pass is the encounter start,
after which every `PlayerAction` (a global cooldown or a cooldown ending, a rage gain, the
stance swap lag, a completed cast) runs the active executors in order: a spell that is not
available counts its status, one whose condition does not hold counts a failed condition, and
one that is cast counts a successful cast. Those counts are the executor statistics.

## The Warrior rotations

`warrior/` holds the six ClassicSim rotations (`DWFury`, `DWFuryConservative`, `DWFuryHSFocus`,
`2hFury`, `Arms`, `Prot`) under their original names, with two translations the port needs:

- Overpower readiness: ClassicSim modelled it as an "Overpower Buff"; here it is the combo
  point the target's dodge grants (`data/spells/overrides/warrior.yaml`), so
  `buff_duration "Overpower Buff" is true | greater N` is `variable "combo_points" greater 0`
  and `is false` is `variable "combo_points" eq 0`.
- The rage dump before stancing (`spell "Mainhand Attack" less 1.5`) reads the swing timer;
  auto attacks are not spells here, so it is `variable "time_remaining_swing" less 1.5`.

Every Warrior rotation keeps Sunder Armor up (`buff_stacks "Sunder Armor" less 5` or under 4 s left).
A selected external debuff (`debuffs:` in a character setup) stands in for the character's own
debuff of the same name: conditions on the own one read the external one (5 stacks, permanent),
and the own one is not applied while it is up, so the raid's Sunder Armor is never sundered over.

The trinket and item-use lines are kept: they link only when the item is equipped and are
skipped otherwise. An item's use is named after its *spell*, not the item, so four lines were
renamed (the item is in a comment): Manual Crowd Pummeler → `Haste`, Zandalarian Hero
Medallion → `Restless Strength`, Diamond Flask → `CHUG! CHUG! CHUG! CHUG!`, Cloudkeeper
Legplates → `Heaven's Blessing`. Uses the sim cannot run yet (Restless Strength and CHUG need a
`DUMMY` script, Badge of the Swarmguard is a proc aura while its buff is up) are not given to
the character, so their lines stay unlinked.
