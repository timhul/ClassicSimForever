//! Named settings: alternatives to the default behavior, given on the command line as
//! `--setting name:value,name:value` (the flag may be repeated).
//!
//! [`parse_setting_pairs`] splits one flag value into its pairs; [`SimSettings::apply_settings`]
//! applies all of them at once (their order does not matter) and rejects unknown names,
//! duplicates, values that do not parse or are out of range, and settings given without the one
//! they depend on; [`SimSettings::named_settings`] lists the non-default ones back, for output.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::rage_formula::{RageFormula, SigmoidParams, SigmoidParamsError};
use crate::sim_settings::SimSettings;

/// A known setting: its name, what it takes and what it depends on.
pub struct NamedSetting {
    pub name: &'static str,
    pub help: &'static str,
    pub kind: SettingKind,
    /// The setting it is given only with (the sigmoid knobs need the sigmoid formula).
    pub requires: Option<SettingRequirement>,
}

/// What a setting's value is, with its default (what the setting is when not given).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SettingKind {
    /// One of `options`.
    Choice {
        options: &'static [&'static str],
        default: &'static str,
    },
    /// A number; `whole` when it has no fraction and is not negative.
    Number { default: f64, whole: bool },
}

/// Another setting's value, which a setting depends on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SettingRequirement {
    pub name: &'static str,
    pub value: &'static str,
}

const SIGMOID: Option<SettingRequirement> = Some(SettingRequirement {
    name: "rage_formula",
    value: "marrow_sigmoid",
});

/// Every setting [`SimSettings::apply_settings`] knows.
pub const NAMED_SETTINGS: [NamedSetting; 7] = [
    NamedSetting {
        name: "rage_formula",
        help: "white swing rage: forever (default) or marrow_sigmoid",
        kind: SettingKind::Choice {
            options: &["forever", "marrow_sigmoid"],
            default: "forever",
        },
        requires: None,
    },
    NamedSetting {
        name: "sigmoid_floor",
        help: "marrow_sigmoid: extra rage per minute with a bad weapon (default 0)",
        kind: SettingKind::Number {
            default: 0.0,
            whole: false,
        },
        requires: SIGMOID,
    },
    NamedSetting {
        name: "sigmoid_ceiling",
        help: "marrow_sigmoid: extra rage per minute where the curve levels off (default 46)",
        kind: SettingKind::Number {
            default: 46.0,
            whole: false,
        },
        requires: SIGMOID,
    },
    NamedSetting {
        name: "sigmoid_midpoint",
        help: "marrow_sigmoid: main-hand weapon DPS where the curve climbs fastest (default 58)",
        kind: SettingKind::Number {
            default: 58.0,
            whole: false,
        },
        requires: SIGMOID,
    },
    NamedSetting {
        name: "sigmoid_width",
        help: "marrow_sigmoid: how spread out the climb is, in weapon DPS (default 3.8)",
        kind: SettingKind::Number {
            default: 3.8,
            whole: false,
        },
        requires: SIGMOID,
    },
    NamedSetting {
        name: "initial_rage",
        help: "rage at the start of every iteration, capped at the maximum (default 0)",
        kind: SettingKind::Number {
            default: 0.0,
            whole: true,
        },
        requires: None,
    },
    NamedSetting {
        name: "target_start_health_percent",
        help: "target health in percent when the fight starts, 1-100; it falls to 0 at the end,                which places the execute ranges (default 100)",
        kind: SettingKind::Number {
            default: 100.0,
            whole: true,
        },
        requires: None,
    },
];

/// The names of [`NAMED_SETTINGS`], comma-separated.
pub fn known_setting_names() -> String {
    NAMED_SETTINGS
        .iter()
        .map(|setting| setting.name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// A named setting is malformed or cannot apply.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SettingError {
    #[error("empty setting in `{0}`: expected name:value pairs separated by commas")]
    Empty(String),
    #[error("setting `{0}` has no value: expected name:value")]
    MissingValue(String),
    #[error("unknown setting `{name}` (known: {known})")]
    Unknown { name: String, known: String },
    #[error("setting `{0}` is given more than once")]
    Duplicate(String),
    #[error("setting {name}: `{value}` is not {expected}")]
    InvalidValue {
        name: String,
        value: String,
        expected: &'static str,
    },
    #[error("setting `{name}` needs {requires}")]
    Requires {
        name: String,
        requires: &'static str,
    },
    #[error(transparent)]
    Sigmoid(#[from] SigmoidParamsError),
}

/// Splits `text` (`name:value,name:value`) into its pairs, trimmed. The value is everything
/// after the first `:`.
pub fn parse_setting_pairs(text: &str) -> Result<Vec<(String, String)>, SettingError> {
    text.split(',')
        .map(|pair| {
            let pair = pair.trim();
            if pair.is_empty() {
                return Err(SettingError::Empty(text.to_string()));
            }
            let (name, value) = pair
                .split_once(':')
                .ok_or_else(|| SettingError::MissingValue(pair.to_string()))?;
            let (name, value) = (name.trim(), value.trim());
            if name.is_empty() {
                return Err(SettingError::Empty(text.to_string()));
            }
            if value.is_empty() {
                return Err(SettingError::MissingValue(name.to_string()));
            }
            Ok((name.to_string(), value.to_string()))
        })
        .collect()
}

/// The pairs of one `--setting` value (see [`parse_setting_pairs`]), as a command-line value
/// type.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingPairs(pub Vec<(String, String)>);

impl std::str::FromStr for SettingPairs {
    type Err = SettingError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        parse_setting_pairs(text).map(SettingPairs)
    }
}

const MARROW_SIGMOID: &str = "rage_formula:marrow_sigmoid";

impl SimSettings {
    /// Applies the named settings `pairs` (from [`parse_setting_pairs`], possibly several flags
    /// concatenated). Nothing changes when one is rejected.
    pub fn apply_settings(&mut self, pairs: &[(String, String)]) -> Result<(), SettingError> {
        let mut values = BTreeMap::new();
        for (name, value) in pairs {
            if !NAMED_SETTINGS.iter().any(|setting| setting.name == name) {
                return Err(SettingError::Unknown {
                    name: name.clone(),
                    known: known_setting_names(),
                });
            }
            if values.insert(name.as_str(), value.as_str()).is_some() {
                return Err(SettingError::Duplicate(name.clone()));
            }
        }

        let mut formula = match values.get("rage_formula") {
            None => self.rage_formula,
            Some(&"forever") => RageFormula::Forever,
            Some(&"marrow_sigmoid") => match self.rage_formula {
                sigmoid @ RageFormula::MarrowSigmoid(_) => sigmoid,
                RageFormula::Forever => RageFormula::MarrowSigmoid(SigmoidParams::default()),
            },
            Some(value) => {
                return Err(SettingError::InvalidValue {
                    name: "rage_formula".to_string(),
                    value: value.to_string(),
                    expected: "forever or marrow_sigmoid",
                });
            }
        };
        for (name, value) in &values {
            let Some(knob) = name.strip_prefix("sigmoid_") else {
                continue;
            };
            let RageFormula::MarrowSigmoid(params) = &mut formula else {
                return Err(SettingError::Requires {
                    name: name.to_string(),
                    requires: MARROW_SIGMOID,
                });
            };
            let number = value
                .parse::<f64>()
                .map_err(|_| SettingError::InvalidValue {
                    name: name.to_string(),
                    value: value.to_string(),
                    expected: "a number",
                })?;
            match knob {
                "floor" => params.floor = number,
                "ceiling" => params.ceiling = number,
                "midpoint" => params.midpoint = number,
                "width" => params.width = number,
                _ => unreachable!("every sigmoid_ setting is a knob"),
            }
        }
        if let RageFormula::MarrowSigmoid(params) = &formula {
            params.validate()?;
        }
        let initial_rage = match values.get("initial_rage") {
            None => self.initial_rage,
            Some(value) => value.parse().map_err(|_| SettingError::InvalidValue {
                name: "initial_rage".to_string(),
                value: value.to_string(),
                expected: "a whole number of rage",
            })?,
        };

        let target_start_health_percent = match values.get("target_start_health_percent") {
            None => self.target_start_health_percent,
            Some(value) => value
                .parse()
                .ok()
                .filter(|percent| (1..=100).contains(percent))
                .ok_or_else(|| SettingError::InvalidValue {
                    name: "target_start_health_percent".to_string(),
                    value: value.to_string(),
                    expected: "a whole percentage from 1 to 100",
                })?,
        };

        self.rage_formula = formula;
        self.initial_rage = initial_rage;
        self.target_start_health_percent = target_start_health_percent;
        Ok(())
    }

    /// Applies every `--setting` flag's pairs together (see
    /// [`apply_settings`](Self::apply_settings)).
    pub fn apply_setting_flags(&mut self, flags: &[SettingPairs]) -> Result<(), SettingError> {
        let pairs: Vec<_> = flags.iter().flat_map(|flag| flag.0.clone()).collect();
        self.apply_settings(&pairs)
    }

    /// The named settings that differ from the default, as `(name, value)` pairs in the order
    /// of [`NAMED_SETTINGS`] (a non-default formula lists all its knobs).
    pub fn named_settings(&self) -> Vec<(&'static str, String)> {
        let mut settings = match self.rage_formula {
            RageFormula::Forever => Vec::new(),
            RageFormula::MarrowSigmoid(params) => vec![
                ("rage_formula", self.rage_formula.name().to_string()),
                ("sigmoid_floor", params.floor.to_string()),
                ("sigmoid_ceiling", params.ceiling.to_string()),
                ("sigmoid_midpoint", params.midpoint.to_string()),
                ("sigmoid_width", params.width.to_string()),
            ],
        };
        if self.initial_rage != 0 {
            settings.push(("initial_rage", self.initial_rage.to_string()));
        }
        if self.target_start_health_percent != 100 {
            settings.push((
                "target_start_health_percent",
                self.target_start_health_percent.to_string(),
            ));
        }
        settings
    }

    /// [`named_settings`](Self::named_settings) as `name:value,name:value`, the form
    /// `--setting` takes; `None` when every setting is the default.
    pub fn named_settings_text(&self) -> Option<String> {
        let settings = self.named_settings();
        (!settings.is_empty()).then(|| {
            settings
                .iter()
                .map(|(name, value)| format!("{name}:{value}"))
                .collect::<Vec<_>>()
                .join(",")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kinds' defaults are the defaults the sim runs with, and the help says the same.
    #[test]
    fn the_settings_defaults_are_the_sims() {
        let params = SigmoidParams::default();
        let defaults = SimSettings::default();
        for setting in &NAMED_SETTINGS {
            let (default, text) = match setting.kind {
                SettingKind::Choice { options, default } => {
                    assert!(options.contains(&default), "{}", setting.name);
                    (None, default.to_string())
                }
                SettingKind::Number { default, .. } => (Some(default), default.to_string()),
            };
            assert!(
                setting.help.contains(&format!("{text} (default)"))
                    || setting.help.contains(&format!("(default {text})")),
                "{}: {}",
                setting.name,
                setting.help
            );
            let expected = match setting.name {
                "sigmoid_floor" => Some(params.floor),
                "sigmoid_ceiling" => Some(params.ceiling),
                "sigmoid_midpoint" => Some(params.midpoint),
                "sigmoid_width" => Some(params.width),
                "initial_rage" => Some(f64::from(defaults.initial_rage)),
                "target_start_health_percent" => {
                    Some(f64::from(defaults.target_start_health_percent))
                }
                _ => None,
            };
            assert_eq!(default, expected, "{}", setting.name);
            if let Some(requires) = setting.requires {
                assert!(
                    NAMED_SETTINGS
                        .iter()
                        .any(|other| other.name == requires.name
                            && matches!(other.kind, SettingKind::Choice { options, .. }
                            if options.contains(&requires.value))),
                    "{}",
                    setting.name
                );
            }
        }
        assert_eq!(defaults.rage_formula, RageFormula::Forever);
    }

    fn pairs(text: &str) -> Vec<(String, String)> {
        parse_setting_pairs(text).unwrap()
    }

    fn applied(text: &str) -> Result<SimSettings, SettingError> {
        let mut settings = SimSettings::default();
        settings.apply_settings(&pairs(text))?;
        Ok(settings)
    }

    #[test]
    fn pairs_are_split_on_commas_and_the_first_colon() {
        assert_eq!(
            pairs(" rage_formula : marrow_sigmoid , sigmoid_width:3.8"),
            vec![
                ("rage_formula".to_string(), "marrow_sigmoid".to_string()),
                ("sigmoid_width".to_string(), "3.8".to_string()),
            ]
        );
        assert_eq!(pairs("a:b:c"), vec![("a".to_string(), "b:c".to_string())]);
    }

    #[test]
    fn malformed_pairs_are_rejected() {
        assert!(matches!(
            parse_setting_pairs(""),
            Err(SettingError::Empty(_))
        ));
        assert!(matches!(
            parse_setting_pairs("a:1,,b:2"),
            Err(SettingError::Empty(_))
        ));
        assert!(matches!(
            parse_setting_pairs(":1"),
            Err(SettingError::Empty(_))
        ));
        assert_eq!(
            parse_setting_pairs("rage_formula"),
            Err(SettingError::MissingValue("rage_formula".to_string()))
        );
        assert_eq!(
            parse_setting_pairs("rage_formula:"),
            Err(SettingError::MissingValue("rage_formula".to_string()))
        );
    }

    #[test]
    fn the_marrow_sigmoid_takes_its_knobs() {
        let settings = applied(
            "rage_formula:marrow_sigmoid,sigmoid_floor:5,sigmoid_ceiling:60,\
             sigmoid_midpoint:65.5,sigmoid_width:2",
        )
        .unwrap();
        assert_eq!(
            settings.rage_formula,
            RageFormula::MarrowSigmoid(SigmoidParams {
                floor: 5.0,
                ceiling: 60.0,
                midpoint: 65.5,
                width: 2.0,
            })
        );
        assert_eq!(
            settings.named_settings_text().unwrap(),
            "rage_formula:marrow_sigmoid,sigmoid_floor:5,sigmoid_ceiling:60,\
             sigmoid_midpoint:65.5,sigmoid_width:2"
        );
    }

    #[test]
    fn unset_knobs_keep_the_chapters_values() {
        let settings = applied("sigmoid_ceiling:30,rage_formula:marrow_sigmoid").unwrap();
        assert_eq!(
            settings.rage_formula,
            RageFormula::MarrowSigmoid(SigmoidParams {
                ceiling: 30.0,
                ..SigmoidParams::default()
            })
        );
    }

    #[test]
    fn repeated_flags_merge() {
        let mut settings = SimSettings::default();
        let mut all = pairs("rage_formula:marrow_sigmoid");
        all.extend(pairs("sigmoid_width:5"));
        settings.apply_settings(&all).unwrap();
        let RageFormula::MarrowSigmoid(params) = settings.rage_formula else {
            panic!("sigmoid expected");
        };
        assert_eq!(params.width, 5.0);
        // A later application keeps the formula and changes one knob.
        settings.apply_settings(&pairs("sigmoid_floor:2")).unwrap();
        let RageFormula::MarrowSigmoid(params) = settings.rage_formula else {
            panic!("sigmoid expected");
        };
        assert_eq!((params.floor, params.width), (2.0, 5.0));
    }

    #[test]
    fn the_default_lists_nothing() {
        let settings = applied("rage_formula:forever").unwrap();
        assert_eq!(settings.rage_formula, RageFormula::Forever);
        assert!(settings.named_settings().is_empty());
        assert_eq!(settings.named_settings_text(), None);
    }

    #[test]
    fn initial_rage_is_a_whole_number() {
        let settings = applied("initial_rage:50").unwrap();
        assert_eq!(settings.initial_rage, 50);
        assert_eq!(settings.sim_params().initial_rage, 50);
        assert_eq!(settings.named_settings_text().unwrap(), "initial_rage:50");
        assert_eq!(
            applied("rage_formula:marrow_sigmoid,initial_rage:20")
                .unwrap()
                .named_settings()
                .last(),
            Some(&("initial_rage", "20".to_string()))
        );
        for bad in ["initial_rage:-1", "initial_rage:12.5", "initial_rage:lots"] {
            assert!(
                matches!(applied(bad), Err(SettingError::InvalidValue { .. })),
                "{bad}"
            );
        }
        assert!(
            applied("initial_rage:0")
                .unwrap()
                .named_settings()
                .is_empty()
        );
    }

    #[test]
    fn target_start_health_is_a_percentage() {
        let settings = applied("target_start_health_percent:30").unwrap();
        assert_eq!(settings.target_start_health_percent, 30);
        assert_eq!(settings.sim_params().target_start_health, 0.3);
        assert_eq!(
            settings.named_settings_text().unwrap(),
            "target_start_health_percent:30"
        );
        assert_eq!(
            applied("target_start_health_percent:1")
                .unwrap()
                .target_start_health_percent,
            1
        );
        for bad in [
            "target_start_health_percent:0",
            "target_start_health_percent:101",
            "target_start_health_percent:-5",
            "target_start_health_percent:12.5",
            "target_start_health_percent:half",
        ] {
            assert!(
                matches!(applied(bad), Err(SettingError::InvalidValue { .. })),
                "{bad}"
            );
        }
        assert!(
            applied("target_start_health_percent:100")
                .unwrap()
                .named_settings()
                .is_empty()
        );
    }

    #[test]
    fn bad_settings_are_rejected_and_change_nothing() {
        let unknown = applied("rage_formula:marrow_sigmoid,sigmoid_celing:46").unwrap_err();
        assert_eq!(
            unknown,
            SettingError::Unknown {
                name: "sigmoid_celing".to_string(),
                known: known_setting_names(),
            }
        );
        assert!(unknown.to_string().contains("sigmoid_ceiling"));
        assert_eq!(
            applied("sigmoid_width:1,sigmoid_width:2").unwrap_err(),
            SettingError::Duplicate("sigmoid_width".to_string())
        );
        assert!(matches!(
            applied("rage_formula:classic").unwrap_err(),
            SettingError::InvalidValue { .. }
        ));
        assert!(matches!(
            applied("rage_formula:marrow_sigmoid,sigmoid_floor:low").unwrap_err(),
            SettingError::InvalidValue { .. }
        ));
        assert_eq!(
            applied("sigmoid_floor:0").unwrap_err(),
            SettingError::Requires {
                name: "sigmoid_floor".to_string(),
                requires: MARROW_SIGMOID,
            }
        );
        assert_eq!(
            applied("rage_formula:forever,sigmoid_floor:0").unwrap_err(),
            SettingError::Requires {
                name: "sigmoid_floor".to_string(),
                requires: MARROW_SIGMOID,
            }
        );
        assert_eq!(
            applied("rage_formula:marrow_sigmoid,sigmoid_width:-1").unwrap_err(),
            SettingError::Sigmoid(SigmoidParamsError::Width(-1.0))
        );

        let mut settings = SimSettings::default();
        assert!(
            settings
                .apply_settings(&pairs("rage_formula:marrow_sigmoid,sigmoid_width:0"))
                .is_err()
        );
        assert_eq!(settings.rage_formula, RageFormula::Forever);
    }
}
