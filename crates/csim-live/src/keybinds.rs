//! The keybinds of a player playing a character from the keyboard: a YAML map of spell name to
//! binding, or of macro name to macro, in the order of the file.
//!
//! ```yaml
//! Bloodthirst: 1
//! Whirlwind: '2'
//! Heroic Strike: Q
//! Execute: Shift+E
//! Recklessness: Ctrl+Shift+Alt+F1
//! Burst:                  # a macro: the game's /cast lines (see Character::queue_macro)
//!   hotkey: T
//!   cast:
//!     - Bloodrage
//!     - Bloodthirst
//! ```
//!
//! A binding is `[Ctrl+][Shift+][Alt+]<key>`, the modifiers in any order and case. The key is a
//! letter, a digit, `F1`..`F12` or the name of a key (`Space`, `Tab`, `Minus`, `Numpad1`, ...:
//! the browser's `KeyboardEvent.code` without its `Key` / `Digit` prefix). Bindings are
//! normalised to `Ctrl+Shift+Alt+KEY` (the modifiers present, in that order), the form the page
//! builds from a key press.

use std::path::Path;

use serde::Deserialize;

/// A spell or a macro bound to a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keybind {
    /// The spell's name, or the macro's.
    pub name: String,
    /// Normalised, see the module documentation.
    pub binding: String,
    /// The spells it casts: the one, or the macro's entries in order.
    pub spells: Vec<String>,
    pub is_macro: bool,
}

/// A macro as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MacroSpec {
    hotkey: serde_yaml::Value,
    cast: Vec<String>,
}

/// The named keys, as `KeyboardEvent.code` names them (without a `Key` / `Digit` prefix).
const NAMED_KEYS: &[&str] = &[
    "Space",
    "Tab",
    "Enter",
    "Backspace",
    "Escape",
    "Minus",
    "Equal",
    "BracketLeft",
    "BracketRight",
    "Semicolon",
    "Quote",
    "Backquote",
    "Backslash",
    "Comma",
    "Period",
    "Slash",
    "Insert",
    "Delete",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Numpad0",
    "Numpad1",
    "Numpad2",
    "Numpad3",
    "Numpad4",
    "Numpad5",
    "Numpad6",
    "Numpad7",
    "Numpad8",
    "Numpad9",
    "NumpadAdd",
    "NumpadSubtract",
    "NumpadMultiply",
    "NumpadDivide",
    "NumpadDecimal",
];

/// Loads the keybinds of `path`.
///
/// # Errors
/// The file cannot be read or is not a map of names to bindings or macros, a binding is
/// malformed, a macro casts nothing, or two keybinds share a binding.
pub fn load(path: &Path) -> Result<Vec<Keybind>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// The keybinds of the YAML `text`; see [`load`].
///
/// # Errors
/// See [`load`].
pub fn parse(text: &str) -> Result<Vec<Keybind>, String> {
    let map: serde_yaml::Mapping = serde_yaml::from_str(text)
        .map_err(|error| format!("not a map of spell to key: {error}"))?;
    let mut keybinds: Vec<Keybind> = Vec::new();
    for (name, value) in map {
        let name = scalar(&name).ok_or("a spell name is not text")?;
        let (binding, spells, is_macro) = match value {
            serde_yaml::Value::Mapping(_) => {
                let spec: MacroSpec = serde_yaml::from_value(value)
                    .map_err(|error| format!("{name}: not a macro: {error}"))?;
                if spec.cast.is_empty() {
                    return Err(format!("{name}: the macro casts nothing"));
                }
                let binding = scalar(&spec.hotkey)
                    .ok_or_else(|| format!("{name}: the hotkey is not text"))?;
                (binding, spec.cast, true)
            }
            value => {
                let binding =
                    scalar(&value).ok_or_else(|| format!("{name}: the binding is not text"))?;
                (binding, vec![name.clone()], false)
            }
        };
        let binding = normalize(&binding).map_err(|error| format!("{name}: {error}"))?;
        if let Some(other) = keybinds.iter().find(|keybind| keybind.binding == binding) {
            return Err(format!("{name}: {binding} is {}'s already", other.name));
        }
        if keybinds.iter().any(|keybind| keybind.name == name) {
            return Err(format!("{name}: bound twice"));
        }
        keybinds.push(Keybind {
            name,
            binding,
            spells,
            is_macro,
        });
    }
    Ok(keybinds)
}

/// A string or a number (`Bloodthirst: 1`) as text.
fn scalar(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(text) => Some(text.clone()),
        serde_yaml::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// `binding` as `Ctrl+Shift+Alt+KEY`; see the module documentation.
///
/// # Errors
/// No key, more than one, an unknown key, or a modifier twice.
pub fn normalize(binding: &str) -> Result<String, String> {
    let (mut ctrl, mut shift, mut alt) = (false, false, false);
    let mut key: Option<String> = None;
    for part in binding.split('+').map(str::trim) {
        let modifier = match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Some(&mut ctrl),
            "shift" => Some(&mut shift),
            "alt" => Some(&mut alt),
            _ => None,
        };
        if let Some(modifier) = modifier {
            if *modifier {
                return Err(format!("{binding:?}: {part} twice"));
            }
            *modifier = true;
            continue;
        }
        if key.is_some() {
            return Err(format!("{binding:?}: more than one key"));
        }
        key =
            Some(normalize_key(part).ok_or_else(|| format!("{binding:?}: unknown key {part:?}"))?);
    }
    let key = key.ok_or_else(|| format!("{binding:?}: no key"))?;
    let modifiers = [(ctrl, "Ctrl+"), (shift, "Shift+"), (alt, "Alt+")];
    let prefix: String = modifiers
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| *name)
        .collect();
    Ok(prefix + &key)
}

/// A letter or digit, `F1`..`F12` or a named key, in its canonical case.
fn normalize_key(key: &str) -> Option<String> {
    let mut chars = key.chars();
    if let (Some(single), None) = (chars.next(), chars.next())
        && single.is_ascii_alphanumeric()
    {
        return Some(single.to_ascii_uppercase().to_string());
    }
    if let Some(number) = key.strip_prefix(['F', 'f'])
        && let Ok(number) = number.parse::<u32>()
        && (1..=12).contains(&number)
    {
        return Some(format!("F{number}"));
    }
    NAMED_KEYS
        .iter()
        .find(|name| name.eq_ignore_ascii_case(key))
        .map(|name| (*name).to_owned())
}

#[cfg(test)]
mod tests;
