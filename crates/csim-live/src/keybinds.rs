//! The keybinds of a player playing a character from the keyboard: a YAML map of spell name to
//! binding, in the order of the file.
//!
//! ```yaml
//! Bloodthirst: 1
//! Whirlwind: '2'
//! Heroic Strike: Q
//! Execute: Shift+E
//! Recklessness: Ctrl+Shift+Alt+F1
//! ```
//!
//! A binding is `[Ctrl+][Shift+][Alt+]<key>`, the modifiers in any order and case. The key is a
//! letter, a digit, `F1`..`F12` or the name of a key (`Space`, `Tab`, `Minus`, `Numpad1`, ...:
//! the browser's `KeyboardEvent.code` without its `Key` / `Digit` prefix). Bindings are
//! normalised to `Ctrl+Shift+Alt+KEY` (the modifiers present, in that order), the form the page
//! builds from a key press.

use std::path::Path;

/// One spell bound to a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keybind {
    pub spell: String,
    /// Normalised, see the module documentation.
    pub binding: String,
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
/// The file cannot be read or is not a map of names to bindings, a binding is malformed, or
/// two spells share a binding.
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
    for (spell, binding) in map {
        let spell = scalar(&spell).ok_or("a spell name is not text")?;
        let binding =
            scalar(&binding).ok_or_else(|| format!("{spell}: the binding is not text"))?;
        let binding = normalize(&binding).map_err(|error| format!("{spell}: {error}"))?;
        if let Some(other) = keybinds.iter().find(|keybind| keybind.binding == binding) {
            return Err(format!("{spell}: {binding} is {}'s already", other.spell));
        }
        keybinds.push(Keybind { spell, binding });
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
