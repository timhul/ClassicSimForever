//! How landed white swings generate rage.
//!
//! [`RageFormula::Forever`] is the game: a fixed amount per second of base weapon speed (see
//! [`swing_rage`](crate::spell::auto_attack::swing_rage)), whatever the gear. The other
//! formulas are proposals, chosen with a setting (`--setting rage_formula:...`), never the
//! default.
//!
//! [`RageFormula::MarrowSigmoid`] is the sigmoid of Marrow's Eternal Compendium of
//! Dragonslaying, "Rage Design Proposals" (5.4-5.6,
//! <https://ppach-warriorcompendium.share.connect.posit.cloud/rage-proposals.html>): on top of
//! Forever's rate, an extra amount of rage per minute that follows an S-shaped curve of the
//! main-hand weapon's DPS. As the chapter's fight simulation does, every landed white swing's
//! Forever rage is scaled by the same factor, so that on paper (every swing landing) the rate
//! becomes Forever's plus the extra (see [`RageFormula::swing_rage_factor`]).

use serde::{Deserialize, Serialize};

/// The rage formula of landed white swings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "formula")]
pub enum RageFormula {
    /// The game: a fixed rate per second of base weapon speed.
    #[default]
    Forever,
    /// Forever's rate plus a sigmoid of the main-hand weapon DPS.
    MarrowSigmoid(SigmoidParams),
}

/// The four knobs of [`RageFormula::MarrowSigmoid`]:
/// `extra(d) = floor + (ceiling - floor) / (1 + e^(-(d - midpoint) / width))` extra rage per
/// minute over Forever's, for a main-hand weapon of `d` DPS.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SigmoidParams {
    /// Extra rage per minute with a bad weapon.
    pub floor: f64,
    /// Extra rage per minute where the curve levels off.
    pub ceiling: f64,
    /// Main-hand weapon DPS where the curve climbs fastest.
    pub midpoint: f64,
    /// How spread out the climb is, in main-hand weapon DPS.
    pub width: f64,
}

impl Default for SigmoidParams {
    /// The chapter's curve: from Forever's rate to Classic's at a 69.25 DPS weapon (+46 rage
    /// per minute), climbing fastest at a phase 2 weapon (58 DPS).
    fn default() -> Self {
        Self {
            floor: 0.0,
            ceiling: 46.0,
            midpoint: 58.0,
            width: 3.8,
        }
    }
}

/// A sigmoid knob is out of range.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SigmoidParamsError {
    #[error("sigmoid_{name} must be a finite number, got {value}")]
    NotFinite { name: &'static str, value: f64 },
    #[error("sigmoid_width must be above 0, got {0}")]
    Width(f64),
}

impl SigmoidParams {
    /// Extra rage per minute over Forever's for a main-hand weapon of `weapon_dps`.
    pub fn extra_rage_per_minute(&self, weapon_dps: f64) -> f64 {
        self.floor
            + (self.ceiling - self.floor)
                / (1.0 + (-(weapon_dps - self.midpoint) / self.width).exp())
    }

    /// Every knob finite, the width positive.
    pub fn validate(&self) -> Result<(), SigmoidParamsError> {
        for (name, value) in [
            ("floor", self.floor),
            ("ceiling", self.ceiling),
            ("midpoint", self.midpoint),
            ("width", self.width),
        ] {
            if !value.is_finite() {
                return Err(SigmoidParamsError::NotFinite { name, value });
            }
        }
        if self.width <= 0.0 {
            return Err(SigmoidParamsError::Width(self.width));
        }
        Ok(())
    }
}

impl RageFormula {
    /// Extra rage per minute over Forever's for a main-hand weapon of `weapon_dps` (0 for
    /// Forever).
    pub fn extra_rage_per_minute(&self, weapon_dps: f64) -> f64 {
        match self {
            RageFormula::Forever => 0.0,
            RageFormula::MarrowSigmoid(params) => params.extra_rage_per_minute(weapon_dps),
        }
    }

    /// The factor a landed white swing's Forever rage is multiplied by: `1 + (extra / 60) /
    /// nominal`, where `nominal` is the character's Forever white rage per second with every
    /// swing landing (6.92 dual wielding with Dual Wield Specialization 5/5). Exactly 1 for
    /// Forever, or without a nominal rate.
    pub fn swing_rage_factor(&self, weapon_dps: f64, nominal_rage_per_second: f64) -> f64 {
        match self {
            RageFormula::Forever => 1.0,
            RageFormula::MarrowSigmoid(_) if nominal_rage_per_second <= 0.0 => 1.0,
            RageFormula::MarrowSigmoid(_) => (1.0
                + self.extra_rage_per_minute(weapon_dps) / 60.0 / nominal_rage_per_second)
                .max(0.0),
        }
    }

    /// The setting value naming the formula (`rage_formula:<name>`).
    pub fn name(&self) -> &'static str {
        match self {
            RageFormula::Forever => "forever",
            RageFormula::MarrowSigmoid(_) => "marrow_sigmoid",
        }
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;

    /// Forever's rate dual wielding with Dual Wield Specialization 5/5: 3.46 per hand.
    const DW_DWS: f64 = 6.92;

    #[test]
    fn the_default_sigmoid_reproduces_the_chapters_table() {
        // Table 5.2, "Proposal: sigmoid": rage per second at each phase's weapon.
        let sigmoid = SigmoidParams::default();
        for (dps, rate) in [
            (42.0, 6.93),
            (51.0, 7.02),
            (58.0, 7.30),
            (65.5, 7.60),
            (73.0, 7.68),
        ] {
            let paper = DW_DWS + sigmoid.extra_rage_per_minute(dps) / 60.0;
            assert_abs_diff_eq!(paper, rate, epsilon = 0.01);
        }
        // The slider table: extra rage per minute, from the page's unrounded knobs (the
        // sliders show C = 46 and w = 3.8 rounded), so within a rage per minute.
        for (dps, extra) in [
            (42.0, 1.0),
            (51.0, 6.0),
            (58.0, 23.0),
            (65.5, 41.0),
            (73.0, 46.0),
        ] {
            assert_abs_diff_eq!(sigmoid.extra_rage_per_minute(dps), extra, epsilon = 1.0);
        }
    }

    #[test]
    fn the_factor_scales_the_nominal_rate_to_the_target() {
        let formula = RageFormula::MarrowSigmoid(SigmoidParams::default());
        let factor = formula.swing_rage_factor(58.0, DW_DWS);
        assert_abs_diff_eq!(DW_DWS * factor, DW_DWS + 23.0 / 60.0, epsilon = 1e-9);
        assert_eq!(formula.swing_rage_factor(58.0, 0.0), 1.0);
    }

    #[test]
    fn forever_adds_nothing() {
        assert_eq!(RageFormula::default(), RageFormula::Forever);
        assert_eq!(RageFormula::Forever.extra_rage_per_minute(80.0), 0.0);
        assert_eq!(RageFormula::Forever.swing_rage_factor(80.0, DW_DWS), 1.0);
    }

    #[test]
    fn the_floor_and_ceiling_bound_the_curve() {
        let sigmoid = SigmoidParams {
            floor: -10.0,
            ceiling: 30.0,
            midpoint: 50.0,
            width: 2.0,
        };
        assert_abs_diff_eq!(sigmoid.extra_rage_per_minute(0.0), -10.0, epsilon = 1e-6);
        assert_abs_diff_eq!(sigmoid.extra_rage_per_minute(50.0), 10.0, epsilon = 1e-9);
        assert_abs_diff_eq!(sigmoid.extra_rage_per_minute(200.0), 30.0, epsilon = 1e-6);
    }

    #[test]
    fn knobs_are_validated() {
        assert_eq!(SigmoidParams::default().validate(), Ok(()));
        let width = SigmoidParams {
            width: 0.0,
            ..SigmoidParams::default()
        };
        assert_eq!(width.validate(), Err(SigmoidParamsError::Width(0.0)));
        let ceiling = SigmoidParams {
            ceiling: f64::INFINITY,
            ..SigmoidParams::default()
        };
        assert!(matches!(
            ceiling.validate(),
            Err(SigmoidParamsError::NotFinite {
                name: "ceiling",
                ..
            })
        ));
    }
}
