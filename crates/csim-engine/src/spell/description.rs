//! A spell's description (`Spell.Description_lang`) as the game's tooltip shows it: its `$`
//! tokens replaced by the spell's values.
//!
//! The tokens covered are those the talent spells use:
//! - `$s1`, `$m1` (and `$S1`, `$M1`): effect 1's base points; `$12345s1` another spell's.
//! - `$/10;s1`, `$*2;m1`: a value divided or multiplied.
//! - `$o1` the periodic total, `$t1` the period in seconds, `$a1` the radius, `$x1` the chain
//!   targets.
//! - `$d` the duration (`10 sec`, `2 min`), `$h` the proc chance, `$n` the proc charges, `$u`
//!   the maximum stacks, `$r` the range.
//! - `${$m1/-1000}.2`: arithmetic over tokens and numbers (`+ - * /`, parentheses), with an
//!   optional number of decimals.
//! - `$lpoint:points;` (`$L` too): the singular after a 1, else the plural.
//!
//! A value standing alone is shown without its sign ("Reduces the cost by $s1" of −10 reads
//! "by 10"), as the game does; in `${...}` the sign counts. A token that cannot be resolved (an
//! unknown spell, a missing effect, a kind not listed here) stays as written, so a gap shows
//! instead of a wrong number. Line breaks come out as `\n`.

use super::record::{EffectRecord, SpellRecord};

/// The description of `record`, the effect base points of `values` (`(effect index, value)`,
/// a talent rank's) replacing the record's own. `lookup` gives the other spells a token names.
pub fn describe(
    record: &SpellRecord,
    values: &[(u32, f64)],
    lookup: &dyn Fn(u32) -> Option<SpellRecord>,
) -> String {
    Resolver {
        own: record,
        values,
        lookup,
    }
    .run(&record.description.replace("\r\n", "\n"))
}

struct Resolver<'a> {
    own: &'a SpellRecord,
    values: &'a [(u32, f64)],
    lookup: &'a dyn Fn(u32) -> Option<SpellRecord>,
}

/// A value token: an optional spell id, its kind letter and an effect number (1 when absent).
struct Token {
    spell: Option<u32>,
    kind: char,
    effect: u32,
}

/// What a value token gives.
enum Value {
    Number(f64),
    /// A duration in milliseconds (`$d`): text alone, seconds in arithmetic.
    Duration(i32),
}

impl Resolver<'_> {
    fn run(&self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut last: Option<f64> = None;
        let mut i = 0;
        while i < chars.len() {
            if chars[i] != '$' {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            match self.resolve(&chars, i + 1, last) {
                Some((text, number, next)) => {
                    out.push_str(&text);
                    if number.is_some() {
                        last = number;
                    }
                    i = next;
                }
                None => {
                    out.push('$');
                    i += 1;
                }
            }
        }
        out
    }

    /// The token after a `$` at `at`: its text, its number (for a later plural) and where the
    /// text goes on.
    fn resolve(
        &self,
        chars: &[char],
        at: usize,
        last: Option<f64>,
    ) -> Option<(String, Option<f64>, usize)> {
        match *chars.get(at)? {
            '{' => {
                let close = at + chars[at..].iter().position(|&c| c == '}')?;
                let inner: String = chars[at + 1..close].iter().collect();
                let value = self.evaluate(&inner)?;
                let mut next = close + 1;
                let mut decimals = None;
                if chars.get(next) == Some(&'.')
                    && let Some(digit) = chars.get(next + 1).and_then(|c| c.to_digit(10))
                {
                    decimals = Some(digit as usize);
                    next += 2;
                }
                let text = match decimals {
                    Some(decimals) => format!("{value:.decimals$}"),
                    None => number(value),
                };
                Some((text, Some(value), next))
            }
            operator @ ('/' | '*') => {
                let semicolon = at + chars[at..].iter().position(|&c| c == ';')?;
                let factor: f64 = chars[at + 1..semicolon]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .ok()?;
                let (token, next) = token(chars, semicolon + 1)?;
                let Value::Number(value) = self.value(&token)? else {
                    return None;
                };
                let value = if operator == '/' {
                    value / factor
                } else {
                    value * factor
                };
                Some((number(value.abs()), Some(value.abs()), next))
            }
            'l' | 'L' => {
                let colon = at + chars[at..].iter().position(|&c| c == ':')?;
                let semicolon = colon + chars[colon..].iter().position(|&c| c == ';')?;
                let words = if last.is_some_and(|n| (n.abs() - 1.0).abs() < 1e-9) {
                    &chars[at + 1..colon]
                } else {
                    &chars[colon + 1..semicolon]
                };
                Some((words.iter().collect(), None, semicolon + 1))
            }
            _ => {
                let (token, next) = token(chars, at)?;
                match self.value(&token)? {
                    Value::Number(value) => Some((number(value.abs()), Some(value.abs()), next)),
                    Value::Duration(ms) => Some((duration(ms), None, next)),
                }
            }
        }
    }

    fn value(&self, token: &Token) -> Option<Value> {
        let other;
        let record = match token.spell {
            Some(id) if id != self.own.id => {
                other = (self.lookup)(id)?;
                &other
            }
            _ => self.own,
        };
        let own = std::ptr::eq(record, self.own);
        let index = token.effect.checked_sub(1)?;
        let effect = || record.effects.iter().find(|effect| effect.index == index);
        let base = || {
            let substituted = own
                .then(|| self.values.iter().find(|(i, _)| *i == index))
                .flatten()
                .map(|(_, value)| *value);
            substituted.or_else(|| effect().map(|effect| f64::from(effect.base_points)))
        };
        let value = match token.kind {
            's' | 'S' | 'm' | 'M' => base()?,
            'o' | 'O' => {
                let period = effect()?.aura_period_ms;
                let duration = record.duration_ms.filter(|&ms| ms > 0)?;
                if period == 0 {
                    return None;
                }
                base()? * f64::from(u32::try_from(duration).ok()? / period)
            }
            't' | 'T' => f64::from(effect()?.aura_period_ms) / 1000.0,
            // A spell whose radius sits on another effect than the token names (Piercing
            // Howl's `$a2` with a single effect): the spell's radius.
            'a' | 'A' => {
                let radius = |effect: &EffectRecord| {
                    effect.radius_yd.into_iter().find(|&radius| radius > 0.0)
                };
                let radius = effect()
                    .and_then(radius)
                    .or_else(|| record.effects.iter().find_map(radius))?;
                f64::from(radius)
            }
            'x' | 'X' => f64::from(effect()?.chain_targets),
            'd' | 'D' => return record.duration_ms.map(Value::Duration),
            'h' | 'H' => f64::from(record.aura_options.proc_chance),
            'n' | 'N' => f64::from(record.aura_options.proc_charges),
            'u' | 'U' => f64::from(record.aura_options.max_stacks),
            'r' | 'R' => f64::from(record.range_yd),
            _ => return None,
        };
        Some(Value::Number(value))
    }

    /// The value of `${expression}`: tokens and numbers with `+ - * /` and parentheses.
    fn evaluate(&self, expression: &str) -> Option<f64> {
        let chars: Vec<char> = expression.chars().filter(|c| !c.is_whitespace()).collect();
        let mut parser = Parser {
            chars: &chars,
            at: 0,
            resolver: self,
        };
        let value = parser.sum()?;
        (parser.at == chars.len() && value.is_finite()).then_some(value)
    }
}

/// The value token at `at` (no `$`): `[spell id]kind[effect]`, and where the text goes on.
fn token(chars: &[char], at: usize) -> Option<(Token, usize)> {
    let mut i = at;
    while chars.get(i).is_some_and(char::is_ascii_digit) {
        i += 1;
    }
    let spell = if i > at {
        Some(chars[at..i].iter().collect::<String>().parse().ok()?)
    } else {
        None
    };
    let kind = *chars.get(i).filter(|c| c.is_ascii_alphabetic())?;
    i += 1;
    let takes_effect = matches!(kind.to_ascii_lowercase(), 's' | 'm' | 'o' | 't' | 'a' | 'x');
    let mut effect = 1;
    if takes_effect && let Some(digit) = chars.get(i).and_then(|c| c.to_digit(10)) {
        effect = digit;
        i += 1;
    }
    Some((
        Token {
            spell,
            kind,
            effect,
        },
        i,
    ))
}

/// A recursive descent over `${...}`.
struct Parser<'a, 'r> {
    chars: &'a [char],
    at: usize,
    resolver: &'a Resolver<'r>,
}

impl Parser<'_, '_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn sum(&mut self) -> Option<f64> {
        let mut value = self.product()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.at += 1;
            let rhs = self.product()?;
            value = if op == '+' { value + rhs } else { value - rhs };
        }
        Some(value)
    }

    fn product(&mut self) -> Option<f64> {
        let mut value = self.factor()?;
        while let Some(op @ ('*' | '/')) = self.peek() {
            self.at += 1;
            let rhs = self.factor()?;
            value = if op == '*' { value * rhs } else { value / rhs };
        }
        Some(value)
    }

    fn factor(&mut self) -> Option<f64> {
        match self.peek()? {
            '-' => {
                self.at += 1;
                Some(-self.factor()?)
            }
            '(' => {
                self.at += 1;
                let value = self.sum()?;
                (self.peek()? == ')').then(|| self.at += 1)?;
                Some(value)
            }
            '$' => {
                let (token, next) = token(self.chars, self.at + 1)?;
                self.at = next;
                match self.resolver.value(&token)? {
                    Value::Number(value) => Some(value),
                    Value::Duration(ms) => Some(f64::from(ms) / 1000.0),
                }
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = self.at;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                    self.at += 1;
                }
                self.chars[start..self.at]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .ok()
            }
            _ => None,
        }
    }
}

/// `value` as the tooltips print it: a whole number without decimals, else up to two.
fn number(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    let text = format!("{rounded:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// A duration of `ms` milliseconds as the tooltips print it (`6 sec`, `2 min`, `1 hr`); −1 is
/// until cancelled.
fn duration(ms: i32) -> String {
    if ms < 0 {
        return "until cancelled".to_owned();
    }
    let seconds = f64::from(ms) / 1000.0;
    if seconds >= 3600.0 {
        let hours = seconds / 3600.0;
        let unit = if (hours - 1.0).abs() < 1e-9 {
            "hr"
        } else {
            "hrs"
        };
        format!("{} {unit}", number(hours))
    } else if seconds >= 60.0 {
        format!("{} min", number(seconds / 60.0))
    } else {
        format!("{} sec", number(seconds))
    }
}

#[cfg(test)]
mod tests;
