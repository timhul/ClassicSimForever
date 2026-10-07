//! The rotation condition mini-language. Port of the sentence grammar in
//! `Rotation/RotationFileReader.cpp` (`rotation_executor_handler`, `add_type`,
//! `add_logical_connective`, `add_compare_operation`), the grouping in
//! `Rotation::add_conditionals` and the `Rotation/Conditions/*` evaluations.
//!
//! A condition is one sentence per line:
//!
//! ```text
//! <type> "<value>" <comparison>
//! and <type> "<value>" <comparison>
//! or  <type> "<value>" <comparison>
//! ```
//!
//! `or` starts a new group; the sentences of a group are AND-ed and the condition holds when
//! any group holds (no parentheses, no precedence beyond that). Types: `buff_duration`,
//! `buff_stacks`, `spell` (cooldown remaining), `resource`, `variable` (a builtin), `talent`
//! (the points spent in the talent, decided once when the condition is linked). A
//! comparison is `less | leq | eq | geq | greater <number>`, or `is true | is false` for the
//! buff types (up / down). `variable "target_is_type"` is compared by name instead:
//! `eq "<creature type>"` (`eq "giant"`).
//!
//! The parsed [`Condition`] is generic over the buff and spell handle types: the parser
//! produces `Condition<String, String>` (the names as written) and the rotation runtime
//! (Phase 5.3) maps it to the character's `BuffId` / `SpellId` with [`Condition::map`]. Values
//! are read through the [`ConditionContext`] trait, so evaluation can be tested without a
//! character.
//!
//! Deviations from the C++ code, all on purpose:
//! - A sentence that does not parse is an error rather than dropping the executor silently.
//! - `leq` / `geq` include equality (within `almost_equal`) and `eq` is symmetric for every
//!   type; the C++ buff-duration and spell-cooldown conditions treated `leq` as `<`, `geq` as
//!   `>` and `eq` as `lhs - rhs < 1e-6`.
//! - `is true` / `is false` produce [`Test::Is`]; the C++ reader mapped them to `Eq` against
//!   the string `"true"` / `"false"` (both `0.0` as a double), which made them equivalent.
//!   They are only accepted for `buff_duration` and `buff_stacks`, the types that define them.
//! - `resource "Focus"` is accepted alongside Mana / Rage / Energy.
//! - `variable "target_is_type" eq "<creature type>"` is new: the target's creature type.
//! - `talent "<talent>"` is new: the talent's rank (points spent in it).

use std::fmt;

use crate::resource::ResourceType;
use crate::target::CreatureType;

/// Tolerance of the numeric comparisons. Port of `almost_equal` (`Utils/CompareDouble`).
pub const EPSILON: f64 = 0.0001;

/// A numeric comparison. Port of the numeric members of `Comparator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Comparator {
    Less,
    Leq,
    Eq,
    Geq,
    Greater,
}

impl Comparator {
    /// The keyword as written in a rotation file.
    pub fn keyword(self) -> &'static str {
        match self {
            Comparator::Less => "less",
            Comparator::Leq => "leq",
            Comparator::Eq => "eq",
            Comparator::Geq => "geq",
            Comparator::Greater => "greater",
        }
    }

    /// The symbol used in condition descriptions. Port of `Condition::comparator_as_string`.
    pub fn symbol(self) -> &'static str {
        match self {
            Comparator::Less => "<",
            Comparator::Leq => "<=",
            Comparator::Eq => "==",
            Comparator::Geq => ">=",
            Comparator::Greater => ">",
        }
    }

    fn from_keyword(keyword: &str) -> Option<Self> {
        Some(match keyword {
            "less" => Comparator::Less,
            "leq" => Comparator::Leq,
            "eq" => Comparator::Eq,
            "geq" => Comparator::Geq,
            "greater" => Comparator::Greater,
            _ => return None,
        })
    }

    /// Whether `lhs <cmp> rhs`, with equality within [`EPSILON`]. Port of
    /// `ConditionResource::condition_fulfilled` / `ConditionVariableBuiltin::cmp_values`.
    pub fn holds(self, lhs: f64, rhs: f64) -> bool {
        let equal = (lhs - rhs).abs() < EPSILON;
        match self {
            Comparator::Less => lhs < rhs && !equal,
            Comparator::Leq => lhs < rhs || equal,
            Comparator::Eq => equal,
            Comparator::Geq => lhs > rhs || equal,
            Comparator::Greater => lhs > rhs && !equal,
        }
    }
}

/// The right-hand side of a sentence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Test {
    /// `less | leq | eq | geq | greater <number>`.
    Compare(Comparator, f64),
    /// `is true` / `is false`: whether the buff is up (`buff_duration`) or has any stacks
    /// (`buff_stacks`). Port of `Comparator::True` / `Comparator::False`.
    Is(bool),
    /// `eq "<creature type>"`: whether the target is of that type (`target_is_type`).
    IsCreatureType(CreatureType),
}

impl Test {
    /// Whether a buff test holds for a buff that is down: no duration left, no stacks.
    fn holds_for_a_buff_down(self) -> bool {
        match self {
            Test::Compare(cmp, rhs) => cmp.holds(0.0, rhs),
            Test::Is(up) => !up,
            Test::IsCreatureType(_) => unreachable!("only parsed for target_is_type"),
        }
    }
}

/// The builtin variables of `variable "<name>"`. Port of `BuiltinVariables`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinVariable {
    /// Remaining target health as a fraction of the combat length, 1 → 0.
    TargetHealth,
    /// Seconds until the encounter ends.
    TimeRemainingEncounter,
    /// Seconds until the execute phase starts (negative once in it).
    TimeRemainingExecute,
    /// Seconds since the last main hand swing.
    TimeSinceSwing,
    /// Seconds until the next main hand swing (not in C++, whose rotations read the swing
    /// timer through `spell "Mainhand Attack"`; auto attacks are not spells here).
    TimeRemainingSwing,
    /// Seconds since the last auto shot.
    TimeSinceAutoShot,
    /// Melee attack power.
    MeleeAp,
    /// Combo points on the target.
    ComboPoints,
    /// Seconds until the global cooldown ends.
    TimeRemainingGcd,
    /// What the character's resource lacks to its maximum: a mana potion restoring up to 2250
    /// is drunk when `geq 2250`, the same for any gear.
    ResourceMissing,
}

impl BuiltinVariable {
    pub const ALL: [BuiltinVariable; 10] = [
        BuiltinVariable::TargetHealth,
        BuiltinVariable::TimeRemainingEncounter,
        BuiltinVariable::TimeRemainingExecute,
        BuiltinVariable::TimeSinceSwing,
        BuiltinVariable::TimeRemainingSwing,
        BuiltinVariable::TimeSinceAutoShot,
        BuiltinVariable::MeleeAp,
        BuiltinVariable::ComboPoints,
        BuiltinVariable::TimeRemainingGcd,
        BuiltinVariable::ResourceMissing,
    ];

    /// The name as written in a rotation file. Port of
    /// `ConditionVariableBuiltin::get_builtin_variable`.
    pub fn name(self) -> &'static str {
        match self {
            BuiltinVariable::TargetHealth => "target_health",
            BuiltinVariable::TimeRemainingEncounter => "time_remaining_encounter",
            BuiltinVariable::TimeRemainingExecute => "time_remaining_execute",
            BuiltinVariable::TimeSinceSwing => "time_since_swing",
            BuiltinVariable::TimeRemainingSwing => "time_remaining_swing",
            BuiltinVariable::TimeSinceAutoShot => "time_since_auto_shot",
            BuiltinVariable::MeleeAp => "melee_ap",
            BuiltinVariable::ComboPoints => "combo_points",
            BuiltinVariable::TimeRemainingGcd => "time_remaining_gcd",
            BuiltinVariable::ResourceMissing => "resource_missing",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|variable| variable.name() == name)
    }

    /// The description used in rotation statistics. Port of
    /// `ConditionVariableBuiltin::condition_description`.
    fn description(self) -> &'static str {
        match self {
            BuiltinVariable::TargetHealth => "Target Health",
            BuiltinVariable::TimeRemainingEncounter => "Time Remaining Encounter",
            BuiltinVariable::TimeRemainingExecute => "Time Remaining Until Execute",
            BuiltinVariable::TimeSinceSwing => "Time Since Mainhand Swing",
            BuiltinVariable::TimeRemainingSwing => "Time Until Mainhand Swing",
            BuiltinVariable::TimeSinceAutoShot => "Time Since Auto Shot",
            BuiltinVariable::MeleeAp => "Melee Attack Power",
            BuiltinVariable::ComboPoints => "Combo Points",
            BuiltinVariable::TimeRemainingGcd => "Time Remaining GCD",
            BuiltinVariable::ResourceMissing => "Resource Missing",
        }
    }

    /// The unit suffix of the description (`seconds`, `%` or nothing).
    fn unit(self) -> &'static str {
        match self {
            BuiltinVariable::TargetHealth => "%",
            BuiltinVariable::MeleeAp | BuiltinVariable::ComboPoints => "",
            _ => " seconds",
        }
    }
}

/// What a sentence measures. Port of `ConditionType` with its `type_value` resolved.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Measure<B, S> {
    /// `buff_duration "<buff>"`: remaining duration in seconds, 0 when down.
    BuffDuration(B),
    /// `buff_stacks "<buff>"`: current stacks, 0 when down.
    BuffStacks(B),
    /// `spell "<spell>"`: cooldown remaining in seconds.
    SpellCooldown(S),
    /// `resource "<Rage|Mana|Energy|Focus>"`: the current amount.
    Resource(ResourceType),
    /// `variable "<builtin>"`.
    Variable(BuiltinVariable),
    /// `variable "target_is_type"`: the target's creature type, compared by name.
    TargetType,
    /// `talent "<talent>"`: the points spent in the talent. Talents do not change during an
    /// iteration, so [`Condition::map`] decides the sentence: a mapped condition has none.
    Talent(String),
}

/// The variable holding the target's creature type, compared by name rather than number.
const TARGET_TYPE_VARIABLE: &str = "target_is_type";

/// The condition type keywords (`add_type`).
const TYPES: [&str; 6] = [
    "buff_duration",
    "buff_stacks",
    "spell",
    "resource",
    "variable",
    "talent",
];

/// One `<type> "<value>" <comparison>` line. Port of `Sentence` + the `Condition` subclass
/// it becomes.
#[derive(Debug, Clone, PartialEq)]
pub struct Sentence<B, S> {
    pub measure: Measure<B, S>,
    pub test: Test,
}

/// Where the values a condition compares come from. Implemented by the rotation runtime for
/// a character (Phase 5.3) and by mocks in tests.
pub trait ConditionContext<B, S> {
    /// `Buff::time_left`: seconds left, 0 when the buff is down.
    fn buff_time_left(&self, buff: &B) -> f64;
    /// `Buff::is_active`.
    fn buff_is_active(&self, buff: &B) -> bool;
    /// `Buff::get_stacks`, 0 when the buff is down.
    fn buff_stacks(&self, buff: &B) -> u32;
    /// `Spell::get_cooldown_remaining`.
    fn spell_cooldown_remaining(&self, spell: &S) -> f64;
    /// `Character::get_resource_level`.
    fn resource_level(&self, resource: ResourceType) -> u32;
    /// The builtin variable's current value.
    fn variable(&self, variable: BuiltinVariable) -> f64;
    /// The target's creature type.
    fn target_creature_type(&self) -> CreatureType;
}

/// When a condition (or a rotation pass) could next come out differently without an event of
/// its own: time-dependent values move on with the clock, a regenerating resource with its
/// ticks. Everything else only changes through events, after which the question is asked again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NextChange {
    /// Seconds from now after which a time-dependent value may have crossed its threshold;
    /// infinite when none can. Rounded down, never late.
    pub delay: f64,
    /// The lowest level of the watched resource above the current one at which a value may
    /// change.
    pub level: Option<u32>,
}

impl NextChange {
    /// Nothing changes by itself.
    pub const NEVER: NextChange = NextChange {
        delay: f64::INFINITY,
        level: None,
    };
    /// Something may already be different.
    pub const NOW: NextChange = NextChange {
        delay: 0.0,
        level: None,
    };

    pub fn after(delay: f64) -> Self {
        NextChange {
            delay: delay.max(0.0),
            level: None,
        }
    }

    pub fn at_level(level: u32) -> Self {
        NextChange {
            delay: f64::INFINITY,
            level: Some(level),
        }
    }

    /// The earlier of both.
    pub fn or(self, other: NextChange) -> NextChange {
        NextChange {
            delay: self.delay.min(other.delay),
            level: match (self.level, other.level) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
        }
    }
}

/// The resource a [`NextChange`] watches: the one that regenerates on its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Watched {
    pub resource: ResourceType,
    pub max: u32,
    /// Seconds of the encounter (the target's health falls by `1 / encounter_length` per
    /// second).
    pub encounter_length: f64,
}

/// Seconds until `value`, moving by `rate` per second, may cross `rhs`: early by twice the
/// comparison tolerance, infinite when it moves away or `floor` stops it first (a cooldown or a
/// duration counts down to 0).
fn crossing(value: f64, rhs: f64, rate: f64, floor: Option<f64>) -> f64 {
    if floor.is_some_and(|floor| value <= floor) {
        return f64::INFINITY;
    }
    let gap = rhs - value;
    if gap * rate < 0.0 && gap.abs() > 2.0 * EPSILON {
        return f64::INFINITY;
    }
    ((gap.abs() - 2.0 * EPSILON) / rate.abs()).max(0.0)
}

impl<B, S> Sentence<B, S> {
    /// Whether the sentence holds. Port of the `condition_fulfilled` overrides.
    pub fn holds(&self, context: &impl ConditionContext<B, S>) -> bool {
        match (&self.measure, self.test) {
            (Measure::BuffDuration(buff), Test::Is(up)) => context.buff_is_active(buff) == up,
            (Measure::BuffDuration(buff), Test::Compare(cmp, rhs)) => {
                cmp.holds(context.buff_time_left(buff), rhs)
            }
            (Measure::BuffStacks(buff), Test::Is(any)) => (context.buff_stacks(buff) > 0) == any,
            (Measure::BuffStacks(buff), Test::Compare(cmp, rhs)) => {
                cmp.holds(f64::from(context.buff_stacks(buff)), rhs)
            }
            (Measure::SpellCooldown(spell), Test::Compare(cmp, rhs)) => {
                cmp.holds(context.spell_cooldown_remaining(spell), rhs)
            }
            (Measure::Resource(resource), Test::Compare(cmp, rhs)) => {
                cmp.holds(f64::from(context.resource_level(*resource)), rhs)
            }
            (Measure::Variable(variable), Test::Compare(cmp, rhs)) => {
                cmp.holds(context.variable(*variable), rhs)
            }
            (Measure::TargetType, Test::IsCreatureType(creature)) => {
                context.target_creature_type() == creature
            }
            // The parser only produces `Is` for the buff measures, and pairs the target type
            // with creature type tests only.
            (
                Measure::SpellCooldown(_) | Measure::Resource(_) | Measure::Variable(_),
                Test::Is(_),
            ) => {
                unreachable!("`is` tests are only parsed for buff measures")
            }
            (Measure::TargetType, _) | (_, Test::IsCreatureType(_)) => {
                unreachable!("target_is_type is only parsed with `eq \"<creature type>\"`")
            }
            (Measure::Talent(_), _) => {
                unreachable!("talent sentences are decided when the condition is mapped")
            }
        }
    }

    /// When the sentence could next flip by itself (see [`NextChange`]): a duration, a
    /// cooldown or a timer crossing the threshold, the watched resource reaching the level at
    /// which the comparison changes.
    pub fn next_change(
        &self,
        context: &impl ConditionContext<B, S>,
        watched: Watched,
    ) -> NextChange {
        let Test::Compare(cmp, rhs) = self.test else {
            return NextChange::NEVER;
        };
        let delay = match &self.measure {
            Measure::BuffDuration(buff) => {
                crossing(context.buff_time_left(buff), rhs, -1.0, Some(0.0))
            }
            Measure::SpellCooldown(spell) => crossing(
                context.spell_cooldown_remaining(spell),
                rhs,
                -1.0,
                Some(0.0),
            ),
            Measure::Variable(variable) => {
                let value = context.variable(*variable);
                match variable {
                    BuiltinVariable::TargetHealth => {
                        crossing(value, rhs, -1.0 / watched.encounter_length, None)
                    }
                    BuiltinVariable::TimeRemainingEncounter
                    | BuiltinVariable::TimeRemainingExecute => crossing(value, rhs, -1.0, None),
                    BuiltinVariable::TimeRemainingSwing | BuiltinVariable::TimeRemainingGcd => {
                        crossing(value, rhs, -1.0, Some(0.0))
                    }
                    BuiltinVariable::TimeSinceSwing | BuiltinVariable::TimeSinceAutoShot => {
                        crossing(value, rhs, 1.0, None)
                    }
                    BuiltinVariable::MeleeAp | BuiltinVariable::ComboPoints => f64::INFINITY,
                    // The watched resource regenerating: the level at which what it lacks to
                    // the maximum crosses the threshold.
                    BuiltinVariable::ResourceMissing => {
                        let level = context.resource_level(watched.resource);
                        let missing = |at: u32| f64::from(watched.max.saturating_sub(at));
                        let holds = cmp.holds(missing(level), rhs);
                        return (level + 1..=watched.max)
                            .find(|&next| cmp.holds(missing(next), rhs) != holds)
                            .map_or(NextChange::NEVER, NextChange::at_level);
                    }
                }
            }
            Measure::Resource(resource) if *resource == watched.resource => {
                let level = context.resource_level(*resource);
                let holds = cmp.holds(f64::from(level), rhs);
                return (level + 1..=watched.max)
                    .find(|&next| cmp.holds(f64::from(next), rhs) != holds)
                    .map_or(NextChange::NEVER, NextChange::at_level);
            }
            Measure::Resource(_)
            | Measure::BuffStacks(_)
            | Measure::TargetType
            | Measure::Talent(_) => f64::INFINITY,
        };
        NextChange::after(delay)
    }

    /// Maps the buff and spell handles, e.g. names to ids. `None` from the spell closure means
    /// the spell is unknown and the sentence (and its condition) cannot be linked. `None` from
    /// the buff closure means the character can never have the buff: the sentence is then
    /// decided as for a buff that is down (`buff_duration "Eureka!" is false` always holds for
    /// a non-Gnome). A talent sentence is decided by the rank `talent` gives the talent.
    pub fn map<B2, S2>(
        self,
        mut buff: impl FnMut(B) -> Option<B2>,
        mut spell: impl FnMut(S) -> Option<S2>,
        mut talent: impl FnMut(&str) -> u32,
    ) -> Option<Mapped<Sentence<B2, S2>>> {
        let measure = match self.measure {
            Measure::BuffDuration(b) => match buff(b) {
                Some(b) => Measure::BuffDuration(b),
                None => return Some(Mapped::Constant(self.test.holds_for_a_buff_down())),
            },
            Measure::BuffStacks(b) => match buff(b) {
                Some(b) => Measure::BuffStacks(b),
                None => return Some(Mapped::Constant(self.test.holds_for_a_buff_down())),
            },
            Measure::SpellCooldown(s) => Measure::SpellCooldown(spell(s)?),
            Measure::Resource(r) => Measure::Resource(r),
            Measure::Variable(v) => Measure::Variable(v),
            Measure::TargetType => Measure::TargetType,
            Measure::Talent(name) => {
                let Test::Compare(cmp, rhs) = self.test else {
                    unreachable!("talent sentences are only parsed with numeric comparisons")
                };
                return Some(Mapped::Constant(cmp.holds(f64::from(talent(&name)), rhs)));
            }
        };
        Some(Mapped::Linked(Sentence {
            measure,
            test: self.test,
        }))
    }
}

/// A mapped sentence: linked, or decided once and for all because it names a buff the
/// character can never have.
#[derive(Debug, Clone, PartialEq)]
pub enum Mapped<T> {
    Linked(T),
    Constant(bool),
}

impl<B: fmt::Display, S: fmt::Display> fmt::Display for Sentence<B, S> {
    /// The description used in rotation statistics. Port of the `condition_description`
    /// overrides.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (cmp, rhs) = match self.test {
            Test::Compare(cmp, rhs) => (cmp, rhs),
            Test::IsCreatureType(creature) => {
                return write!(f, "Target Type == {}", creature.name());
            }
            Test::Is(value) => {
                return match &self.measure {
                    Measure::BuffDuration(buff) => {
                        write!(
                            f,
                            "{buff} buff {}",
                            if value { "active" } else { "inactive" }
                        )
                    }
                    Measure::BuffStacks(buff) => {
                        write!(
                            f,
                            "{buff} buff stacks {}",
                            if value { "> 0" } else { "== 0" }
                        )
                    }
                    _ => unreachable!("`is` tests are only parsed for buff measures"),
                };
            }
        };
        let symbol = cmp.symbol();
        match &self.measure {
            Measure::BuffDuration(buff) => {
                write!(f, "{buff} buff remaining {symbol} {rhs:.1} seconds")
            }
            Measure::BuffStacks(buff) => write!(f, "{buff} buff stacks {symbol} {rhs:.0}"),
            Measure::SpellCooldown(spell) => {
                write!(f, "{spell} cooldown {symbol} {rhs:.1} seconds")
            }
            Measure::Resource(resource) => {
                write!(f, "{} {symbol} {rhs:.0}", resource_name(*resource))
            }
            Measure::TargetType => unreachable!("target_is_type only has type tests"),
            Measure::Talent(talent) => write!(f, "{talent} talent rank {symbol} {rhs:.0}"),
            Measure::Variable(variable) => {
                let precision = if variable.unit().is_empty() { 0 } else { 1 };
                write!(
                    f,
                    "{} {symbol} {rhs:.*}{}",
                    variable.description(),
                    precision,
                    variable.unit()
                )
            }
        }
    }
}

/// `ConditionResource::name_for_resource`, also what `resource "<value>"` accepts.
fn resource_name(resource: ResourceType) -> &'static str {
    match resource {
        ResourceType::Mana => "Mana",
        ResourceType::Rage => "Rage",
        ResourceType::Energy => "Energy",
        ResourceType::Focus => "Focus",
    }
}

fn resource_from_name(name: &str) -> Option<ResourceType> {
    ResourceType::ALL
        .into_iter()
        .find(|resource| resource_name(*resource).eq_ignore_ascii_case(name))
}

/// A parsed condition: OR-ed groups of AND-ed sentences. Port of
/// `RotationExecutor::condition_groups`.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition<B = String, S = String> {
    groups: Vec<Vec<Sentence<B, S>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message} (in `{text}`)")]
pub struct ConditionParseError {
    /// 1-based line among the non-blank lines of the condition.
    pub line: usize,
    /// The offending line, trimmed.
    pub text: String,
    pub message: String,
}

impl Condition<String, String> {
    /// Parses condition text, one sentence per non-blank line. Port of
    /// `RotationFileReader::rotation_executor_handler` + `Rotation::add_conditionals`.
    pub fn parse(text: &str) -> Result<Self, ConditionParseError> {
        let mut groups: Vec<Vec<Sentence<String, String>>> = Vec::new();
        let lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
        for (index, line) in lines.enumerate() {
            let error = |message: String| ConditionParseError {
                line: index + 1,
                text: line.to_string(),
                message,
            };
            let (connective, sentence) = parse_line(line, index == 0).map_err(error)?;
            match connective {
                Connective::Start | Connective::Or => groups.push(vec![sentence]),
                Connective::And => groups
                    .last_mut()
                    .expect("first line starts a group")
                    .push(sentence),
            }
        }
        if groups.is_empty() {
            return Err(ConditionParseError {
                line: 0,
                text: String::new(),
                message: "condition is empty".to_string(),
            });
        }
        Ok(Condition { groups })
    }
}

impl<B, S> Condition<B, S> {
    /// The OR-ed groups; the sentences within a group are AND-ed.
    pub fn groups(&self) -> &[Vec<Sentence<B, S>>] {
        &self.groups
    }

    /// Every sentence in file order.
    pub fn sentences(&self) -> impl Iterator<Item = &Sentence<B, S>> {
        self.groups.iter().flatten()
    }

    /// Whether any group has all its sentences fulfilled. Port of
    /// `RotationExecutor::attempt_cast`'s loop over `condition_group_fulfilled`.
    pub fn holds(&self, context: &impl ConditionContext<B, S>) -> bool {
        self.groups
            .iter()
            .any(|group| group.iter().all(|sentence| sentence.holds(context)))
    }

    /// The earliest [`Sentence::next_change`] of its sentences: until then, the condition
    /// holds or fails as it does now.
    pub fn next_change(
        &self,
        context: &impl ConditionContext<B, S>,
        watched: Watched,
    ) -> NextChange {
        self.sentences().fold(NextChange::NEVER, |next, sentence| {
            next.or(sentence.next_change(context, watched))
        })
    }

    /// Maps the buff and spell handles of every sentence (see [`Sentence::map`]). A sentence
    /// naming a buff the character can never have, or a talent, is dropped when it always
    /// holds and drops its group when it never does. `None` when a spell is unknown or no
    /// group can hold; a group left empty always holds.
    pub fn map<B2, S2>(
        self,
        mut buff: impl FnMut(B) -> Option<B2>,
        mut spell: impl FnMut(S) -> Option<S2>,
        mut talent: impl FnMut(&str) -> u32,
    ) -> Option<Condition<B2, S2>> {
        let mut groups = Vec::new();
        for group in self.groups {
            let mut linked = Vec::new();
            let mut can_hold = true;
            for sentence in group {
                match sentence.map(&mut buff, &mut spell, &mut talent)? {
                    Mapped::Linked(sentence) => linked.push(sentence),
                    Mapped::Constant(holds) => can_hold &= holds,
                }
            }
            if can_hold {
                groups.push(linked);
            }
        }
        if groups.is_empty() {
            return None;
        }
        Some(Condition { groups })
    }
}

impl<B: fmt::Display, S: fmt::Display> fmt::Display for Condition<B, S> {
    /// One description per line, groups separated by an `OR` line. Port of
    /// `RotationExecutor::get_conditions_string`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut lines: Vec<String> = Vec::new();
        for (index, group) in self.groups.iter().enumerate() {
            if index > 0 {
                lines.push("OR".to_string());
            }
            lines.extend(group.iter().map(ToString::to_string));
        }
        f.write_str(&lines.join("\n"))
    }
}

enum Connective {
    Start,
    And,
    Or,
}

/// Parses `[and|or] <type> "<value>" <comparison>`. The first line carries no connective,
/// every other line must.
fn parse_line(line: &str, first: bool) -> Result<(Connective, Sentence<String, String>), String> {
    // Split on the quotation marks: [connective] type | value | comparison.
    let mut parts = line.splitn(3, '"');
    let head = parts.next().unwrap_or_default().trim();
    let value = parts
        .next()
        .ok_or_else(|| "expected a quoted value".to_string())?
        .trim();
    let tail = parts
        .next()
        .ok_or_else(|| "expected a closing quotation mark".to_string())?
        .trim();
    if value.is_empty() {
        return Err("the quoted value is empty".to_string());
    }

    let mut head_words = head.split_whitespace();
    let (connective, type_word) = match (head_words.next(), head_words.next(), head_words.next()) {
        (Some(type_word), None, None) if first => (Connective::Start, type_word),
        (Some(type_word), None, None) => {
            return Err(format!(
                "expected `and` or `or` before `{type_word}` (every line but the first starts \
                 with a logical connective)"
            ));
        }
        (Some("and"), Some(type_word), None) if !first => (Connective::And, type_word),
        (Some("or"), Some(type_word), None) if !first => (Connective::Or, type_word),
        (Some(word), Some(_), None) if first => {
            return Err(format!(
                "unexpected `{word}` (the first line carries no logical connective)"
            ));
        }
        (Some(word), Some(_), None) => return Err(format!("expected `and` or `or`, got `{word}`")),
        _ => {
            return Err(format!(
                "expected `[and|or] <type>` before the quoted value, got `{head}`"
            ));
        }
    };

    if type_word == "variable" && value == TARGET_TYPE_VARIABLE {
        let creature = parse_creature_type_test(tail)?;
        return Ok((
            connective,
            Sentence {
                measure: Measure::TargetType,
                test: Test::IsCreatureType(creature),
            },
        ));
    }
    let test = parse_test(tail)?;
    let measure = match type_word {
        "buff_duration" => Measure::BuffDuration(value.to_string()),
        "buff_stacks" => Measure::BuffStacks(value.to_string()),
        "spell" => Measure::SpellCooldown(value.to_string()),
        "talent" => Measure::Talent(value.to_string()),
        "resource" => Measure::Resource(resource_from_name(value).ok_or_else(|| {
            format!("unknown resource `{value}` (expected Mana, Rage, Energy or Focus)")
        })?),
        "variable" => Measure::Variable(BuiltinVariable::from_name(value).ok_or_else(|| {
            let names: Vec<&str> = BuiltinVariable::ALL
                .iter()
                .map(|v| v.name())
                .chain([TARGET_TYPE_VARIABLE])
                .collect();
            format!(
                "unknown builtin variable `{value}` (expected one of {})",
                names.join(", ")
            )
        })?),
        other => {
            return Err(format!(
                "unknown condition type `{other}` (expected one of {})",
                TYPES.join(", ")
            ));
        }
    };
    if matches!(test, Test::Is(_))
        && !matches!(measure, Measure::BuffDuration(_) | Measure::BuffStacks(_))
    {
        return Err(format!(
            "`is true` / `is false` only applies to buff_duration and buff_stacks, not \
             {type_word}"
        ));
    }
    Ok((connective, Sentence { measure, test }))
}

/// Parses the `eq "<creature type>"` of `variable "target_is_type"`; the type name is not case
/// sensitive.
fn parse_creature_type_test(text: &str) -> Result<CreatureType, String> {
    let names = || {
        let names: Vec<&str> = CreatureType::ALL.iter().map(|t| t.name()).collect();
        names.join(", ")
    };
    let name = text
        .strip_prefix("eq")
        .map(str::trim_start)
        .and_then(|rest| rest.strip_prefix('"'))
        .and_then(|rest| rest.strip_suffix('"'))
        .filter(|name| !name.contains('"'))
        .ok_or_else(|| {
            format!(
                "expected `eq \"<creature type>\"` after `{TARGET_TYPE_VARIABLE}` (one of {}), \
                 got `{text}`",
                names()
            )
        })?;
    CreatureType::ALL
        .into_iter()
        .find(|t| t.name().eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| {
            format!(
                "unknown creature type `{name}` (expected one of {})",
                names()
            )
        })
}

/// Parses `<comparator> <number>` or `is true|false`. Port of `add_compare_operation`.
fn parse_test(text: &str) -> Result<Test, String> {
    let mut words = text.split_whitespace();
    let (Some(op), Some(rhs), None) = (words.next(), words.next(), words.next()) else {
        return Err(format!(
            "expected `<less|leq|eq|geq|greater> <number>` or `is <true|false>` after the \
             quoted value, got `{text}`"
        ));
    };
    if op == "is" {
        return match rhs {
            "true" => Ok(Test::Is(true)),
            "false" => Ok(Test::Is(false)),
            other => Err(format!(
                "expected `true` or `false` after `is`, got `{other}`"
            )),
        };
    }
    let comparator = Comparator::from_keyword(op).ok_or_else(|| {
        format!("unknown comparator `{op}` (expected less, leq, eq, geq, greater or is)")
    })?;
    let value: f64 = rhs
        .parse()
        .map_err(|_| format!("expected a number after `{op}`, got `{rhs}`"))?;
    if !value.is_finite() {
        return Err(format!(
            "expected a finite number after `{op}`, got `{rhs}`"
        ));
    }
    Ok(Test::Compare(comparator, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A context answering from fixed values, keyed by buff / spell name.
    #[derive(Default)]
    struct Mock {
        buff_time_left: HashMap<String, f64>,
        buff_stacks: HashMap<String, u32>,
        cooldowns: HashMap<String, f64>,
        resources: HashMap<ResourceType, u32>,
        variables: HashMap<BuiltinVariable, f64>,
        target_creature_type: Option<CreatureType>,
    }

    impl ConditionContext<String, String> for Mock {
        fn buff_time_left(&self, buff: &String) -> f64 {
            self.buff_time_left.get(buff).copied().unwrap_or(0.0)
        }
        fn buff_is_active(&self, buff: &String) -> bool {
            self.buff_time_left(buff) > 0.0
        }
        fn buff_stacks(&self, buff: &String) -> u32 {
            self.buff_stacks.get(buff).copied().unwrap_or(0)
        }
        fn spell_cooldown_remaining(&self, spell: &String) -> f64 {
            self.cooldowns.get(spell).copied().unwrap_or(0.0)
        }
        fn resource_level(&self, resource: ResourceType) -> u32 {
            self.resources.get(&resource).copied().unwrap_or(0)
        }
        fn variable(&self, variable: BuiltinVariable) -> f64 {
            self.variables.get(&variable).copied().unwrap_or(0.0)
        }
        fn target_creature_type(&self) -> CreatureType {
            self.target_creature_type.unwrap_or(CreatureType::Dragonkin)
        }
    }

    fn sentence(text: &str) -> Sentence<String, String> {
        let condition = Condition::parse(text).unwrap();
        assert_eq!(condition.groups().len(), 1);
        assert_eq!(condition.groups()[0].len(), 1);
        condition.groups()[0][0].clone()
    }

    fn resource(cmp: Comparator, resource: ResourceType, rhs: f64) -> Sentence<String, String> {
        Sentence {
            measure: Measure::Resource(resource),
            test: Test::Compare(cmp, rhs),
        }
    }

    fn buff_duration(name: &str, cmp: Comparator, rhs: f64) -> Sentence<String, String> {
        Sentence {
            measure: Measure::BuffDuration(name.to_string()),
            test: Test::Compare(cmp, rhs),
        }
    }

    fn variable(v: BuiltinVariable, cmp: Comparator, rhs: f64) -> Sentence<String, String> {
        Sentence {
            measure: Measure::Variable(v),
            test: Test::Compare(cmp, rhs),
        }
    }

    // --- When a condition could change by itself ---

    const WATCHED: Watched = Watched {
        resource: ResourceType::Energy,
        max: 100,
        encounter_length: 300.0,
    };

    #[test]
    fn next_change_of_resource_sentences_is_the_level_that_flips_them() {
        let mut ctx = Mock::default();
        ctx.resources.insert(ResourceType::Energy, 40);
        let next =
            |sentence: Sentence<String, String>, ctx: &Mock| sentence.next_change(ctx, WATCHED);
        let greater = resource(Comparator::Greater, ResourceType::Energy, 80.0);
        assert_eq!(next(greater.clone(), &ctx), NextChange::at_level(81));
        assert_eq!(
            next(resource(Comparator::Less, ResourceType::Energy, 45.0), &ctx),
            NextChange::at_level(45)
        );
        assert_eq!(
            next(resource(Comparator::Geq, ResourceType::Energy, 40.0), &ctx),
            NextChange::NEVER,
            "holds now and on every level above"
        );
        assert_eq!(
            next(resource(Comparator::Eq, ResourceType::Energy, 47.0), &ctx),
            NextChange::at_level(47)
        );
        assert_eq!(
            next(
                resource(Comparator::Greater, ResourceType::Energy, 100.0),
                &ctx
            ),
            NextChange::NEVER,
            "above the maximum"
        );
        assert_eq!(
            next(
                resource(Comparator::Greater, ResourceType::Rage, 10.0),
                &ctx
            ),
            NextChange::NEVER,
            "not the watched resource"
        );
        ctx.resources.insert(ResourceType::Energy, 100);
        assert_eq!(next(greater, &ctx), NextChange::NEVER);
    }

    /// What the watched resource lacks shrinks as it regenerates: `resource_missing geq 50`
    /// stops holding at the level 51, `less 30` starts holding at 71.
    #[test]
    fn next_change_of_resource_missing_is_the_level_that_flips_it() {
        let mut ctx = Mock::default();
        ctx.resources.insert(ResourceType::Energy, 40);
        let next = |text: &str| sentence(text).next_change(&ctx, WATCHED);
        assert_eq!(
            next("variable \"resource_missing\" geq 50"),
            NextChange::at_level(51)
        );
        assert_eq!(
            next("variable \"resource_missing\" less 30"),
            NextChange::at_level(71)
        );
        assert_eq!(
            next("variable \"resource_missing\" geq 0"),
            NextChange::NEVER
        );
    }

    #[test]
    fn next_change_of_time_sentences_is_when_they_cross() {
        let mut ctx = Mock::default();
        ctx.buff_time_left.insert("Slice and Dice".to_string(), 7.0);
        ctx.variables.insert(BuiltinVariable::TargetHealth, 0.5);
        ctx.variables.insert(BuiltinVariable::TimeSinceSwing, 0.5);
        ctx.cooldowns.insert("Adrenaline Rush".to_string(), 20.0);
        let delay = |text: &str| sentence(text).next_change(&ctx, WATCHED).delay;
        let early = 2.0 * EPSILON;
        // A duration falls to the threshold, a little early.
        assert_eq!(
            delay(r#"buff_duration "Slice and Dice" less 2"#),
            5.0 - early
        );
        assert_eq!(
            delay(r#"buff_duration "Slice and Dice" greater 2"#),
            5.0 - early
        );
        // Moving away, or stopped at 0 first.
        assert!(delay(r#"buff_duration "Slice and Dice" less 8"#).is_infinite());
        assert!(delay(r#"buff_duration "Slice and Dice" greater -1"#).is_finite());
        assert!(delay(r#"buff_duration "Expose Armor" less 2"#).is_infinite());
        assert!(delay(r#"buff_duration "Slice and Dice" is true"#).is_infinite());
        assert_eq!(delay(r#"spell "Adrenaline Rush" less 5"#), 15.0 - early);
        // The target loses 1 / 300 of its health per second.
        let health = delay(r#"variable "target_health" less 0.2"#);
        assert!((health - (0.3 - early) * 300.0).abs() < 1e-9, "{health}");
        // Time since the swing rises.
        assert_eq!(
            delay(r#"variable "time_since_swing" greater 1.5"#),
            1.0 - early
        );
        assert!(delay(r#"variable "time_since_swing" less 0.2"#).is_infinite());
        assert!(delay(r#"variable "combo_points" geq 5"#).is_infinite());

        // A condition changes when its first sentence does.
        let condition = Condition::parse(
            r#"buff_duration "Slice and Dice" less 2
               and resource "Energy" greater 80
               or variable "time_since_swing" greater 1.5"#,
        )
        .unwrap();
        ctx.resources.insert(ResourceType::Energy, 40);
        let next = condition.next_change(&ctx, WATCHED);
        assert_eq!(next.delay, 1.0 - early);
        assert_eq!(next.level, Some(81));
    }

    // --- Grammar (port of TestRotationFileReader::test_warrior_dw_fury) ---

    #[test]
    fn berserker_rage_is_one_group_of_one_resource_sentence() {
        let condition = Condition::parse("resource \"Rage\" less 50").unwrap();
        assert_eq!(
            condition.groups(),
            [vec![resource(Comparator::Less, ResourceType::Rage, 50.0)]]
        );
    }

    #[test]
    fn battle_shout_splits_into_two_groups_at_the_or() {
        let condition = Condition::parse(
            "buff_duration \"Battle Shout\" less 3\n\
             or variable \"time_remaining_execute\" less 10\n\
             and variable \"time_remaining_execute\" greater 0\n\
             and buff_duration \"Battle Shout\" less 45\n",
        )
        .unwrap();
        assert_eq!(
            condition.groups(),
            [
                vec![buff_duration("Battle Shout", Comparator::Less, 3.0)],
                vec![
                    variable(
                        BuiltinVariable::TimeRemainingExecute,
                        Comparator::Less,
                        10.0
                    ),
                    variable(
                        BuiltinVariable::TimeRemainingExecute,
                        Comparator::Greater,
                        0.0
                    ),
                    buff_duration("Battle Shout", Comparator::Less, 45.0),
                ],
            ]
        );
    }

    #[test]
    fn heroic_strike_is_one_group_of_two_sentences() {
        let condition = Condition::parse(
            "    variable \"time_remaining_execute\" greater 3   \n\n\
             \t and resource \"Rage\" greater 50",
        )
        .unwrap();
        assert_eq!(
            condition.groups(),
            [vec![
                variable(
                    BuiltinVariable::TimeRemainingExecute,
                    Comparator::Greater,
                    3.0
                ),
                resource(Comparator::Greater, ResourceType::Rage, 50.0),
            ]]
        );
        assert_eq!(condition.sentences().count(), 2);
    }

    #[test]
    fn every_type_comparator_and_value_form_parses() {
        assert_eq!(
            sentence("buff_stacks \"Sunder Armor\" geq 5"),
            Sentence {
                measure: Measure::BuffStacks("Sunder Armor".to_string()),
                test: Test::Compare(Comparator::Geq, 5.0),
            }
        );
        assert_eq!(
            sentence("spell \"Bloodthirst\" greater 1.5"),
            Sentence {
                measure: Measure::SpellCooldown("Bloodthirst".to_string()),
                test: Test::Compare(Comparator::Greater, 1.5),
            }
        );
        assert_eq!(
            sentence("buff_duration \"Death Wish\" is true"),
            Sentence {
                measure: Measure::BuffDuration("Death Wish".to_string()),
                test: Test::Is(true),
            }
        );
        assert_eq!(
            sentence("buff_stacks \"Flurry\" is false"),
            Sentence {
                measure: Measure::BuffStacks("Flurry".to_string()),
                test: Test::Is(false),
            }
        );
        assert_eq!(
            sentence("variable \"time_remaining_execute\" less 0").test,
            Test::Compare(Comparator::Less, 0.0)
        );
        assert_eq!(
            sentence("variable \"melee_ap\" leq 2000"),
            variable(BuiltinVariable::MeleeAp, Comparator::Leq, 2000.0)
        );
        assert_eq!(
            sentence("variable \"target_health\" eq 0.2").test,
            Test::Compare(Comparator::Eq, 0.2)
        );
        // A quoted value may contain an apostrophe.
        assert_eq!(
            sentence("buff_duration \"Slayer's Crest\" is true").measure,
            Measure::BuffDuration("Slayer's Crest".to_string())
        );
        // Resources are matched case-insensitively; Focus is accepted.
        assert_eq!(
            sentence("resource \"energy\" less 40").measure,
            Measure::Resource(ResourceType::Energy)
        );
        assert_eq!(
            sentence("resource \"Focus\" less 40").measure,
            Measure::Resource(ResourceType::Focus)
        );
        assert_eq!(
            sentence("resource \"Mana\" greater 100").measure,
            Measure::Resource(ResourceType::Mana)
        );
    }

    #[test]
    fn every_builtin_variable_round_trips_its_name() {
        for variable in BuiltinVariable::ALL {
            assert_eq!(BuiltinVariable::from_name(variable.name()), Some(variable));
            let text = format!("variable \"{}\" geq 1", variable.name());
            assert_eq!(sentence(&text).measure, Measure::Variable(variable));
        }
        assert_eq!(BuiltinVariable::from_name("swing_timer"), None);
    }

    #[test]
    fn parse_errors_name_the_line_and_the_problem() {
        let error = |text: &str| Condition::parse(text).unwrap_err();

        let e = error("");
        assert_eq!(e.line, 0);
        assert!(e.message.contains("empty"), "{e}");

        let e = error("resource Rage less 50");
        assert_eq!(e.line, 1);
        assert!(e.message.contains("quoted value"), "{e}");

        let e = error("resource \"Rage less 50");
        assert!(e.message.contains("closing quotation"), "{e}");

        let e = error("resource \"\" less 50");
        assert!(e.message.contains("empty"), "{e}");

        let e = error("and resource \"Rage\" less 50");
        assert!(e.message.contains("first line"), "{e}");

        let e = error("resource \"Rage\" less 50\nresource \"Rage\" greater 10");
        assert_eq!(e.line, 2);
        assert_eq!(e.text, "resource \"Rage\" greater 10");
        assert!(e.message.contains("`and` or `or`"), "{e}");

        let e = error("resource \"Rage\" less 50\nnot resource \"Rage\" greater 10");
        assert!(e.message.contains("got `not`"), "{e}");

        let e = error("buff \"Rage\" less 50");
        assert!(e.message.contains("unknown condition type `buff`"), "{e}");

        let e = error("resource \"Runic\" less 50");
        assert!(e.message.contains("unknown resource `Runic`"), "{e}");

        let e = error("variable \"swing_timer\" less 50");
        assert!(
            e.message.contains("unknown builtin variable `swing_timer`"),
            "{e}"
        );
        assert!(e.message.contains("time_since_swing"), "{e}");

        let e = error("spell \"Bloodthirst\"");
        assert!(e.message.contains("after the quoted value"), "{e}");

        let e = error("spell \"Bloodthirst\" greater");
        assert!(e.message.contains("after the quoted value"), "{e}");

        let e = error("spell \"Bloodthirst\" greater 1 2");
        assert!(e.message.contains("after the quoted value"), "{e}");

        let e = error("spell \"Bloodthirst\" gt 1");
        assert!(e.message.contains("unknown comparator `gt`"), "{e}");

        let e = error("spell \"Bloodthirst\" greater one");
        assert!(e.message.contains("expected a number"), "{e}");

        let e = error("spell \"Bloodthirst\" greater NaN");
        assert!(e.message.contains("finite"), "{e}");

        let e = error("buff_duration \"Flurry\" is maybe");
        assert!(e.message.contains("`true` or `false`"), "{e}");

        let e = error("spell \"Bloodthirst\" is true");
        assert!(e.message.contains("only applies to buff_duration"), "{e}");

        let e = error("resource \"Rage\" is false");
        assert!(e.message.contains("only applies to buff_duration"), "{e}");

        assert_eq!(
            e.to_string(),
            "line 1: `is true` / `is false` only applies to buff_duration and buff_stacks, \
             not resource (in `resource \"Rage\" is false`)"
        );
    }

    // --- Comparators ---

    #[test]
    fn comparators_include_equality_within_epsilon() {
        assert!(Comparator::Leq.holds(50.00005, 50.0));
        assert!(Comparator::Geq.holds(49.99995, 50.0));
        assert!(Comparator::Eq.holds(50.00005, 50.0));
        assert!(!Comparator::Less.holds(49.99995, 50.0));
        assert!(!Comparator::Greater.holds(50.00005, 50.0));
        assert!(Comparator::Less.holds(49.9, 50.0));
        assert!(Comparator::Greater.holds(50.1, 50.0));
        assert!(!Comparator::Eq.holds(50.1, 50.0));
    }

    // --- Evaluation (port of TestConditionResource) ---

    fn resource_holds(cmp: Comparator, resource_type: ResourceType, level: u32) -> bool {
        let mut mock = Mock::default();
        mock.resources.insert(resource_type, level);
        resource(cmp, resource_type, 50.0).holds(&mock)
    }

    #[test]
    fn resource_comparisons_against_50() {
        for resource_type in [ResourceType::Rage, ResourceType::Energy, ResourceType::Mana] {
            let expect = |cmp: Comparator, expected: [bool; 5]| {
                for (level, expected) in [0, 49, 50, 51, 100].into_iter().zip(expected) {
                    assert_eq!(
                        resource_holds(cmp, resource_type, level),
                        expected,
                        "{resource_type:?} {level} {cmp:?} 50"
                    );
                }
            };
            expect(Comparator::Less, [true, true, false, false, false]);
            expect(Comparator::Leq, [true, true, true, false, false]);
            expect(Comparator::Eq, [false, false, true, false, false]);
            expect(Comparator::Geq, [false, false, true, true, true]);
            expect(Comparator::Greater, [false, false, false, true, true]);
        }
    }

    // --- Evaluation (port of TestConditionVariableBuiltin) ---

    fn combo_points_hold(cmp: Comparator, points: u32) -> bool {
        let mut mock = Mock::default();
        mock.variables
            .insert(BuiltinVariable::ComboPoints, f64::from(points));
        variable(BuiltinVariable::ComboPoints, cmp, 3.0).holds(&mock)
    }

    #[test]
    fn combo_point_comparisons_against_3() {
        let expect = |cmp: Comparator, expected: [bool; 6]| {
            for (points, expected) in (0..=5).zip(expected) {
                assert_eq!(
                    combo_points_hold(cmp, points),
                    expected,
                    "{points} {cmp:?} 3"
                );
            }
        };
        expect(Comparator::Less, [true, true, true, false, false, false]);
        expect(Comparator::Leq, [true, true, true, true, false, false]);
        expect(Comparator::Eq, [false, false, false, true, false, false]);
        expect(Comparator::Geq, [false, false, false, true, true, true]);
        expect(
            Comparator::Greater,
            [false, false, false, false, true, true],
        );
    }

    fn timer_holds(v: BuiltinVariable, cmp: Comparator, rhs: f64, since: f64) -> bool {
        let mut mock = Mock::default();
        mock.variables.insert(v, since);
        variable(v, cmp, rhs).holds(&mock)
    }

    #[test]
    fn swing_and_auto_shot_timers_less_and_greater() {
        for v in [
            BuiltinVariable::TimeSinceSwing,
            BuiltinVariable::TimeSinceAutoShot,
        ] {
            // (time since the swing, `less 0.2`, `less 0.3`)
            for (since, less_200, less_300) in [
                (0.0, true, true),
                (0.1, true, true),
                (0.19, true, true),
                (0.21, false, true),
                (0.29, false, true),
                (0.31, false, false),
            ] {
                assert_eq!(
                    timer_holds(v, Comparator::Less, 0.2, since),
                    less_200,
                    "{since}"
                );
                assert_eq!(
                    timer_holds(v, Comparator::Less, 0.3, since),
                    less_300,
                    "{since}"
                );
                assert_eq!(
                    timer_holds(v, Comparator::Greater, 0.2, since),
                    !less_200,
                    "{since}"
                );
                assert_eq!(
                    timer_holds(v, Comparator::Greater, 0.3, since),
                    !less_300,
                    "{since}"
                );
            }
        }
    }

    // --- Evaluation of buffs, spells and groups ---

    #[test]
    fn buff_duration_and_stacks_tests() {
        let mut mock = Mock::default();
        mock.buff_time_left.insert("Death Wish".to_string(), 12.5);
        mock.buff_stacks.insert("Sunder Armor".to_string(), 3);

        assert!(sentence("buff_duration \"Death Wish\" is true").holds(&mock));
        assert!(!sentence("buff_duration \"Death Wish\" is false").holds(&mock));
        assert!(sentence("buff_duration \"Overpower Buff\" is false").holds(&mock));
        assert!(!sentence("buff_duration \"Overpower Buff\" is true").holds(&mock));
        assert!(sentence("buff_duration \"Death Wish\" greater 12").holds(&mock));
        assert!(!sentence("buff_duration \"Death Wish\" greater 13").holds(&mock));
        assert!(sentence("buff_duration \"Overpower Buff\" less 3").holds(&mock));

        assert!(sentence("buff_stacks \"Sunder Armor\" is true").holds(&mock));
        assert!(!sentence("buff_stacks \"Sunder Armor\" is false").holds(&mock));
        assert!(sentence("buff_stacks \"Flurry\" is false").holds(&mock));
        assert!(sentence("buff_stacks \"Sunder Armor\" eq 3").holds(&mock));
        assert!(sentence("buff_stacks \"Sunder Armor\" less 5").holds(&mock));
        assert!(!sentence("buff_stacks \"Sunder Armor\" geq 5").holds(&mock));
    }

    #[test]
    fn spell_cooldown_tests() {
        let mut mock = Mock::default();
        mock.cooldowns.insert("Bloodthirst".to_string(), 2.0);
        assert!(sentence("spell \"Bloodthirst\" greater 1.5").holds(&mock));
        assert!(!sentence("spell \"Bloodthirst\" greater 3").holds(&mock));
        assert!(sentence("spell \"Bloodthirst\" eq 2").holds(&mock));
        assert!(sentence("spell \"Whirlwind\" less 0.1").holds(&mock));
    }

    #[test]
    fn groups_are_and_ed_and_or_ed() {
        let condition = Condition::parse(
            "buff_duration \"Battle Shout\" less 3\n\
             or variable \"time_remaining_execute\" less 10\n\
             and variable \"time_remaining_execute\" greater 0\n\
             and buff_duration \"Battle Shout\" less 45\n",
        )
        .unwrap();
        let mut mock = Mock::default();

        // Battle Shout down: the first group holds.
        mock.variables
            .insert(BuiltinVariable::TimeRemainingExecute, 100.0);
        assert!(condition.holds(&mock));

        // Battle Shout up with 30 s, far from execute: neither group.
        mock.buff_time_left.insert("Battle Shout".to_string(), 30.0);
        assert!(!condition.holds(&mock));

        // 5 s to execute: the second group holds.
        mock.variables
            .insert(BuiltinVariable::TimeRemainingExecute, 5.0);
        assert!(condition.holds(&mock));

        // ... unless the shout has more than 45 s left.
        mock.buff_time_left.insert("Battle Shout".to_string(), 50.0);
        assert!(!condition.holds(&mock));

        // Already in execute: the second group's `greater 0` fails.
        mock.buff_time_left.insert("Battle Shout".to_string(), 30.0);
        mock.variables
            .insert(BuiltinVariable::TimeRemainingExecute, -1.0);
        assert!(!condition.holds(&mock));
    }

    #[test]
    fn target_is_type_compares_the_creature_type_by_name() {
        let giant = sentence("variable \"target_is_type\" eq \"giant\"");
        assert_eq!(giant.measure, Measure::TargetType);
        assert_eq!(giant.test, Test::IsCreatureType(CreatureType::Giant));
        assert_eq!(giant.to_string(), "Target Type == Giant");
        assert_eq!(
            sentence("variable \"target_is_type\" eq \"Mechanical\"").test,
            Test::IsCreatureType(CreatureType::Mechanical)
        );

        // Every creature type can be named; only the target's own type holds.
        let mut mock = Mock::default();
        for target in CreatureType::ALL {
            mock.target_creature_type = Some(target);
            for named in CreatureType::ALL {
                let text = format!("variable \"target_is_type\" eq \"{}\"", named.name());
                assert_eq!(
                    sentence(&text).holds(&mock),
                    named == target,
                    "{named:?} vs {target:?}"
                );
            }
        }

        // Spearing Strike's targets as two groups.
        let condition = Condition::parse(
            "variable \"target_is_type\" eq \"giant\"\n\
             or variable \"target_is_type\" eq \"dragonkin\"",
        )
        .unwrap();
        mock.target_creature_type = Some(CreatureType::Dragonkin);
        assert!(condition.holds(&mock));
        mock.target_creature_type = Some(CreatureType::Humanoid);
        assert!(!condition.holds(&mock));
    }

    #[test]
    fn target_is_type_errors() {
        let error = |text: &str| Condition::parse(text).unwrap_err();
        let e = error("variable \"target_is_type\" eq \"ooze\"");
        assert!(e.message.contains("unknown creature type `ooze`"), "{e}");
        assert!(e.message.contains("Beast, Demon, Dragonkin"), "{e}");
        for tail in [
            "eq 1",
            "is true",
            "greater \"giant\"",
            "eq \"giant",
            "eq giant",
        ] {
            let e = error(&format!("variable \"target_is_type\" {tail}"));
            assert!(
                e.message.contains("expected `eq \"<creature type>\"`"),
                "{tail}: {e}"
            );
        }
        let e = error("variable \"target_type\" eq 1");
        assert!(e.message.contains("target_is_type"), "listed: {e}");
        let e = error("variable \"melee_ap\" eq \"giant\"");
        assert!(e.message.contains("expected a number"), "{e}");
    }

    // --- Mapping and descriptions ---

    /// A character without talent points.
    fn no_talents(_: &str) -> u32 {
        0
    }

    #[test]
    fn talent_sentences_are_decided_by_the_talent_rank() {
        let talent = sentence("talent \"Improved Berserker Rage\" greater 0");
        assert_eq!(
            talent,
            Sentence {
                measure: Measure::Talent("Improved Berserker Rage".to_string()),
                test: Test::Compare(Comparator::Greater, 0.0),
            }
        );
        assert_eq!(
            talent.to_string(),
            "Improved Berserker Rage talent rank > 0"
        );
        let e = Condition::parse("talent \"Improved Berserker Rage\" is true").unwrap_err();
        assert!(e.message.contains("not talent"), "{e}");

        let condition = Condition::parse(
            "talent \"Improved Berserker Rage\" greater 0
             and resource \"Rage\" less 50
             or talent \"Death Wish\" eq 1
             and spell \"Bloodthirst\" greater 1.5",
        )
        .unwrap();
        let link = |ranks: &[(&str, u32)]| {
            condition.clone().map(
                |name: String| Some(name),
                |name: String| Some(name),
                |name| {
                    ranks
                        .iter()
                        .find(|(talent, _)| *talent == name)
                        .map_or(0, |(_, rank)| *rank)
                },
            )
        };
        // No talent taken: no group can hold, the condition cannot be linked.
        assert!(link(&[]).is_none());
        // A fulfilled talent sentence drops out of its group, a failed one drops the group.
        assert_eq!(
            link(&[("Improved Berserker Rage", 2)]).unwrap().to_string(),
            "Rage < 50"
        );
        assert_eq!(
            link(&[("Improved Berserker Rage", 1), ("Death Wish", 1)])
                .unwrap()
                .to_string(),
            "Rage < 50
OR
Bloodthirst cooldown > 1.5 seconds"
        );
    }

    #[test]
    fn map_resolves_names_and_fails_on_an_unknown_one() {
        let condition = Condition::parse(
            "buff_duration \"Death Wish\" is true\n\
             and spell \"Bloodthirst\" greater 1.5\n\
             or resource \"Rage\" less 50",
        )
        .unwrap();
        let buffs = |name: String| (name == "Death Wish").then_some(7u32);
        let spells = |name: String| (name == "Bloodthirst").then_some(3u32);
        let mapped = condition.clone().map(buffs, spells, no_talents).unwrap();
        assert_eq!(
            mapped.groups(),
            [
                vec![
                    Sentence {
                        measure: Measure::BuffDuration(7),
                        test: Test::Is(true)
                    },
                    Sentence {
                        measure: Measure::SpellCooldown(3),
                        test: Test::Compare(Comparator::Greater, 1.5)
                    },
                ],
                vec![Sentence {
                    measure: Measure::Resource(ResourceType::Rage),
                    test: Test::Compare(Comparator::Less, 50.0)
                }],
            ]
        );
        // An unknown spell cannot be linked.
        assert!(
            condition
                .clone()
                .map(|name: String| Some(name), |_| None::<u32>, no_talents)
                .is_none()
        );
        // An unknown buff is never up: `is true` fails its group, the other group remains.
        let no_buffs = |_: String| None::<u32>;
        let rage_only = condition
            .map(no_buffs, |name: String| Some(name), no_talents)
            .unwrap();
        assert_eq!(rage_only.to_string(), "Rage < 50");
        assert!(
            Condition::parse("buff_duration \"Eureka!\" is true")
                .unwrap()
                .map(no_buffs, |name: String| Some(name), no_talents)
                .is_none()
        );
        // ... while `is false` always holds and drops out of its group.
        let condition = Condition::parse(
            "spell \"Bloodthirst\" greater 1.5\n\
             and buff_duration \"Eureka!\" is false\n\
             and buff_stacks \"Eureka!\" less 1",
        )
        .unwrap();
        let mapped = condition.map(no_buffs, spells, no_talents).unwrap();
        assert_eq!(
            mapped.groups(),
            [vec![Sentence {
                measure: Measure::SpellCooldown(3),
                test: Test::Compare(Comparator::Greater, 1.5)
            }]]
        );
        // A group left empty always holds.
        let always = Condition::parse("buff_duration \"Eureka!\" less 3")
            .unwrap()
            .map(no_buffs, |name: String| Some(name), no_talents)
            .unwrap();
        assert_eq!(always.groups(), [Vec::<Sentence<u32, String>>::new()]);
    }

    #[test]
    fn descriptions_follow_the_statistics_format() {
        let condition = Condition::parse(
            "buff_duration \"Battle Shout\" less 3\n\
             or variable \"time_remaining_execute\" less 10\n\
             and buff_stacks \"Sunder Armor\" geq 5\n\
             and spell \"Bloodthirst\" greater 1.5\n\
             and resource \"Rage\" greater 50\n\
             and variable \"melee_ap\" leq 2000\n\
             and variable \"target_health\" less 0.2\n\
             and buff_duration \"Death Wish\" is true\n\
             and buff_stacks \"Flurry\" is false",
        )
        .unwrap();
        assert_eq!(
            condition.to_string(),
            "Battle Shout buff remaining < 3.0 seconds\n\
             OR\n\
             Time Remaining Until Execute < 10.0 seconds\n\
             Sunder Armor buff stacks >= 5\n\
             Bloodthirst cooldown > 1.5 seconds\n\
             Rage > 50\n\
             Melee Attack Power <= 2000\n\
             Target Health < 0.2%\n\
             Death Wish buff active\n\
             Flurry buff stacks == 0"
        );
    }
}
