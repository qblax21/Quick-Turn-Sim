// SPDX-License-Identifier: Apache-2.0
//! Validation harness.
//!
//! Runs scripted manoeuvres on the headless simulation and compares the
//! results with target values stored in `data/validation/<car>.toml`. Every
//! comparison reports the simulated value, the target, the error in
//! percent, the tolerance and where the target came from.
//!
//! The harness never calls a model "realistic". A pass against a target
//! whose confidence is `placeholder` or `estimated` means only that the
//! placeholder parameters land near a class-typical number. Only a pass
//! against `measured` targets says anything about a real vehicle, and the
//! report states which kind each line is.

pub mod manoeuvres;

use qts_core::provenance::Confidence;
use qts_sim::DataRoot;
use serde::Deserialize;
use std::fmt;

/// A quantity the harness can measure.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Highest lateral acceleration held on a constant-radius circle, g.
    SkidpadLateralG,
    /// Distance to stop from 60 mph with full brake applied at 60 mph, m.
    #[serde(rename = "braking_distance_60_0")]
    BrakingDistance60To0,
    /// Time from a standing start to 60 mph, s.
    #[serde(rename = "accel_0_60")]
    Accel0To60,
    /// Highest speed reached at full throttle on the level, m/s.
    TopSpeed,
    /// Road-wheel steer angle per unit lateral acceleration on a
    /// constant-radius circle, beyond the low-speed angle, deg/g.
    UndersteerGradient,
}

impl Metric {
    /// The name used in target files.
    pub fn name(self) -> &'static str {
        match self {
            Metric::SkidpadLateralG => "skidpad_lateral_g",
            Metric::BrakingDistance60To0 => "braking_distance_60_0",
            Metric::Accel0To60 => "accel_0_60",
            Metric::TopSpeed => "top_speed",
            Metric::UndersteerGradient => "understeer_gradient",
        }
    }

    /// The unit results are reported in.
    pub fn unit(self) -> &'static str {
        match self {
            Metric::SkidpadLateralG => "g",
            Metric::BrakingDistance60To0 => "m",
            Metric::Accel0To60 => "s",
            Metric::TopSpeed => "m/s",
            Metric::UndersteerGradient => "deg/g",
        }
    }
}

/// One target value.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// What is measured.
    pub metric: Metric,
    /// The value to compare against.
    pub value: f64,
    /// Unit of `value`; must be the metric's reporting unit.
    pub unit: String,
    /// Largest acceptable error, percent of the target value.
    pub tolerance_percent: f64,
    /// Where the target came from.
    pub source: String,
    /// `placeholder`, `estimated` or `measured`.
    pub confidence: String,
}

impl Target {
    /// The parsed confidence level.
    pub fn confidence(&self) -> Option<Confidence> {
        Confidence::parse(&self.confidence)
    }
}

/// The targets for one car, as written in `data/validation`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TargetSet {
    /// Car identifier.
    pub car: String,
    /// Track the manoeuvres run on. It must have open tarmac around the
    /// spawn point and a painted circle marker for the skidpad.
    pub track: String,
    /// Tire temperature at the start of each manoeuvre, °C.
    pub tire_temp_c: f64,
    /// Free-text note shown at the top of the report.
    #[serde(default)]
    pub note: String,
    /// The targets.
    #[serde(rename = "target")]
    pub targets: Vec<Target>,
}

impl TargetSet {
    /// Loads `data/validation/<id>.toml`.
    pub fn load(root: &DataRoot, id: &str) -> Result<Self, String> {
        let (set, _): (TargetSet, u64) = root
            .load_plain("validation", id)
            .map_err(|e| e.to_string())?;
        set.check()?;
        Ok(set)
    }

    /// Checks units, tolerances and confidence labels.
    pub fn check(&self) -> Result<(), String> {
        if self.targets.is_empty() {
            return Err(format!("target set for `{}` has no targets", self.car));
        }
        for t in &self.targets {
            let name = t.metric.name();
            if t.unit != t.metric.unit() {
                return Err(format!(
                    "target `{name}`: unit is `{}` but this metric is reported in `{}`",
                    t.unit,
                    t.metric.unit()
                ));
            }
            if !(t.value.is_finite() && t.value != 0.0) {
                return Err(format!(
                    "target `{name}`: value must be finite and non-zero"
                ));
            }
            if !(t.tolerance_percent.is_finite() && t.tolerance_percent > 0.0) {
                return Err(format!(
                    "target `{name}`: tolerance_percent must be positive"
                ));
            }
            if t.source.trim().is_empty() {
                return Err(format!("target `{name}`: source must not be empty"));
            }
            if t.confidence().is_none() {
                return Err(format!(
                    "target `{name}`: confidence `{}` is not one of placeholder, estimated, measured",
                    t.confidence
                ));
            }
        }
        Ok(())
    }
}

/// A value measured from the simulation.
#[derive(Clone, Debug, PartialEq)]
pub struct Measurement {
    /// What was measured.
    pub metric: Metric,
    /// The value, in the metric's reporting unit.
    pub value: f64,
    /// How the manoeuvre was driven, in a few words. A metric can reflect
    /// the test driver as much as the car, so this is always reported.
    pub method: String,
}

/// One target compared with the simulation.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// The target.
    pub target: Target,
    /// The simulated value.
    pub simulated: f64,
    /// `(simulated − target) / |target| × 100`.
    pub error_percent: f64,
    /// True when `|error_percent| ≤ tolerance_percent`.
    pub pass: bool,
    /// How the manoeuvre was driven.
    pub method: String,
}

/// Compares a measurement with its target.
pub fn compare(target: &Target, measurement: &Measurement) -> Comparison {
    let error_percent = (measurement.value - target.value) / target.value.abs() * 100.0;
    Comparison {
        target: target.clone(),
        simulated: measurement.value,
        error_percent,
        pass: error_percent.is_finite() && error_percent.abs() <= target.tolerance_percent,
        method: measurement.method.clone(),
    }
}

/// The outcome of validating one car.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// Car identifier.
    pub car: String,
    /// Car display name.
    pub car_name: String,
    /// Track identifier.
    pub track: String,
    /// Tire start temperature, °C.
    pub tire_temp_c: f64,
    /// Note from the target file.
    pub note: String,
    /// One entry per target.
    pub comparisons: Vec<Comparison>,
    /// Manoeuvres that could not be run, as `(metric name, reason)`.
    pub failures: Vec<(String, String)>,
}

impl Report {
    /// True when every target was measured and is within tolerance.
    pub fn passed(&self) -> bool {
        self.failures.is_empty() && self.comparisons.iter().all(|c| c.pass)
    }

    /// True when at least one target is not a measurement.
    pub fn has_unmeasured_targets(&self) -> bool {
        self.comparisons
            .iter()
            .any(|c| c.target.confidence() != Some(Confidence::Measured))
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Validation: {} ({}) on {}, tires starting at {:.0} degC",
            self.car_name, self.car, self.track, self.tire_temp_c
        )?;
        if !self.note.trim().is_empty() {
            writeln!(f, "Note: {}", self.note.trim())?;
        }
        if self.has_unmeasured_targets() {
            writeln!(
                f,
                "Targets marked placeholder or estimated are NOT measurements. Passing them says nothing\nabout how close the model is to a real vehicle."
            )?;
        }
        writeln!(
            f,
            "{:<24} {:>11} {:>11} {:>9} {:>7}  {:<6} {:<12}",
            "metric", "simulated", "target", "error", "tol", "result", "target is"
        )?;
        for c in &self.comparisons {
            let unit = c.target.metric.unit();
            writeln!(
                f,
                "{:<24} {:>11} {:>11} {:>8.1}% {:>6.1}%  {:<6} {:<12}",
                c.target.metric.name(),
                format!("{:.3} {unit}", c.simulated),
                format!("{:.3} {unit}", c.target.value),
                c.error_percent,
                c.target.tolerance_percent,
                if c.pass { "PASS" } else { "FAIL" },
                c.target.confidence,
            )?;
        }
        for (metric, reason) in &self.failures {
            writeln!(f, "{metric:<24} could not be measured: {reason}")?;
        }
        writeln!(f, "How each value was obtained:")?;
        for c in &self.comparisons {
            writeln!(f, "  {:<24} {}", c.target.metric.name(), c.method)?;
            writeln!(f, "  {:<24} target source: {}", "", c.target.source)?;
        }
        write!(
            f,
            "Overall: {}",
            if self.passed() { "PASS" } else { "FAIL" }
        )
    }
}

/// Runs every target in a set and compares.
pub fn validate(root: &DataRoot, set: &TargetSet) -> Result<Report, String> {
    set.check()?;
    let car_name = root
        .load_car(&set.car)
        .map_err(|e| e.to_string())?
        .value
        .name;
    let mut cache = manoeuvres::Cache::default();
    let mut comparisons = Vec::new();
    let mut failures = Vec::new();
    for target in &set.targets {
        match manoeuvres::measure(root, set, target.metric, &mut cache) {
            Ok(m) => comparisons.push(compare(target, &m)),
            Err(reason) => failures.push((target.metric.name().to_string(), reason)),
        }
    }
    Ok(Report {
        car: set.car.clone(),
        car_name,
        track: set.track.clone(),
        tire_temp_c: set.tire_temp_c,
        note: set.note.clone(),
        comparisons,
        failures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(value: f64, tolerance: f64) -> Target {
        Target {
            metric: Metric::Accel0To60,
            value,
            unit: "s".into(),
            tolerance_percent: tolerance,
            source: "test".into(),
            confidence: "placeholder".into(),
        }
    }

    fn measured(value: f64) -> Measurement {
        Measurement {
            metric: Metric::Accel0To60,
            value,
            method: "test".into(),
        }
    }

    #[test]
    fn error_is_signed_percent_of_target() {
        let c = compare(&target(6.0, 10.0), &measured(6.3));
        assert!((c.error_percent - 5.0).abs() < 1e-9);
        assert!(c.pass);
        let c = compare(&target(6.0, 10.0), &measured(5.1));
        assert!((c.error_percent + 15.0).abs() < 1e-9);
        assert!(!c.pass);
    }

    #[test]
    fn tolerance_is_configurable_and_inclusive() {
        assert!(compare(&target(10.0, 5.0), &measured(10.5)).pass);
        assert!(!compare(&target(10.0, 4.9), &measured(10.5)).pass);
        assert!(compare(&target(10.0, 50.0), &measured(14.0)).pass);
        assert!(!compare(&target(10.0, 5.0), &measured(f64::NAN)).pass);
    }

    #[test]
    fn malformed_target_sets_are_rejected() {
        let good = TargetSet {
            car: "c".into(),
            track: "t".into(),
            tire_temp_c: 50.0,
            note: String::new(),
            targets: vec![target(6.0, 10.0)],
        };
        assert!(good.check().is_ok());
        let mut bad = good.clone();
        bad.targets[0].unit = "min".into();
        assert!(bad.check().unwrap_err().contains("unit"));
        let mut bad = good.clone();
        bad.targets[0].tolerance_percent = 0.0;
        assert!(bad.check().is_err());
        let mut bad = good.clone();
        bad.targets[0].confidence = "sure".into();
        assert!(bad.check().unwrap_err().contains("sure"));
        let mut bad = good.clone();
        bad.targets[0].source = " ".into();
        assert!(bad.check().is_err());
        let mut bad = good;
        bad.targets.clear();
        assert!(bad.check().is_err());
    }

    #[test]
    fn report_states_errors_results_and_the_placeholder_warning() {
        let report = Report {
            car: "c".into(),
            car_name: "Car".into(),
            track: "t".into(),
            tire_temp_c: 60.0,
            note: "n".into(),
            comparisons: vec![
                compare(&target(6.0, 10.0), &measured(6.3)),
                compare(&target(6.0, 10.0), &measured(9.0)),
            ],
            failures: vec![],
        };
        let text = report.to_string();
        assert!(text.contains("NOT measurements"));
        assert!(text.contains("5.0%") && text.contains("PASS"));
        assert!(text.contains("50.0%") && text.contains("FAIL"));
        assert!(text.ends_with("Overall: FAIL"));
        assert!(!report.passed());

        let mut all_measured = report.clone();
        all_measured.comparisons.truncate(1);
        all_measured.comparisons[0].target.confidence = "measured".into();
        assert!(all_measured.passed());
        assert!(!all_measured.to_string().contains("NOT measurements"));

        let mut broken = all_measured;
        broken.failures.push(("top_speed".into(), "no path".into()));
        assert!(!broken.passed());
        assert!(broken
            .to_string()
            .contains("could not be measured: no path"));
    }
}
