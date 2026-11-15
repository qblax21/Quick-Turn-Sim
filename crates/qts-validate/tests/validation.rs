// SPDX-License-Identifier: Apache-2.0
//! Runs the validation harness against the shipped target files.
//!
//! The shipped targets are placeholders. These tests passing means the
//! placeholder cars land near class-typical numbers and that the harness
//! works; it is not evidence that the models match any real vehicle.

use qts_core::provenance::Confidence;
use qts_sim::DataRoot;
use qts_validate::{validate, Metric, TargetSet};

fn data() -> DataRoot {
    DataRoot::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data"))
}

const ALL_METRICS: [Metric; 5] = [
    Metric::SkidpadLateralG,
    Metric::BrakingDistance60To0,
    Metric::Accel0To60,
    Metric::TopSpeed,
    Metric::UndersteerGradient,
];

fn run(id: &str) {
    let root = data();
    let set = TargetSet::load(&root, id).unwrap();
    let report = validate(&root, &set).unwrap();
    // Always show the numbers, pass or fail (visible with --nocapture and
    // in the failure output).
    println!("{report}\n");
    assert!(
        report.failures.is_empty(),
        "manoeuvres failed: {:?}",
        report.failures
    );
    for metric in ALL_METRICS {
        let c = report
            .comparisons
            .iter()
            .find(|c| c.target.metric == metric)
            .unwrap_or_else(|| panic!("{id}: no target for {}", metric.name()));
        assert!(c.simulated.is_finite() && c.error_percent.is_finite());
        assert!(!c.method.is_empty());
    }
    let out_of_tolerance: Vec<String> = report
        .comparisons
        .iter()
        .filter(|c| !c.pass)
        .map(|c| {
            format!(
                "{}: simulated {:.3}, target {:.3}, error {:+.1}% (tolerance {:.1}%)",
                c.target.metric.name(),
                c.simulated,
                c.target.value,
                c.error_percent,
                c.target.tolerance_percent
            )
        })
        .collect();
    assert!(
        out_of_tolerance.is_empty(),
        "{id} is outside tolerance:\n{}\n\n{report}",
        out_of_tolerance.join("\n")
    );
    let text = report.to_string();
    assert!(text.contains("Overall: PASS"));
    assert!(
        text.contains("NOT measurements"),
        "a report on placeholder targets must say so"
    );
}

#[test]
fn hatch_meets_its_placeholder_targets() {
    run("hatch_tern_gt");
}

#[test]
fn electric_sedan_meets_its_placeholder_targets() {
    run("sedan_halden_e4");
}

#[test]
fn shipped_targets_are_all_labelled_as_not_measured() {
    let root = data();
    for id in root.list("validation").unwrap() {
        let set = TargetSet::load(&root, &id).unwrap();
        assert_eq!(set.car, id, "target file name should match its car");
        for t in &set.targets {
            assert_ne!(
                t.confidence(),
                Some(Confidence::Measured),
                "{id}/{}: no measurement has been supplied to the project yet",
                t.metric.name()
            );
        }
    }
}

#[test]
fn a_target_beyond_tolerance_fails_and_reports_its_error() {
    let root = data();
    let mut set = TargetSet::load(&root, "sedan_halden_e4").unwrap();
    // Keep only the quick braking manoeuvre and demand an impossible stop.
    set.targets
        .retain(|t| t.metric == Metric::BrakingDistance60To0);
    set.targets[0].value = 20.0;
    set.targets[0].tolerance_percent = 5.0;
    let report = validate(&root, &set).unwrap();
    assert!(!report.passed());
    let c = &report.comparisons[0];
    assert!(!c.pass);
    let expected = (c.simulated - 20.0) / 20.0 * 100.0;
    assert!((c.error_percent - expected).abs() < 1e-9);
    assert!(c.error_percent > 50.0, "error {}", c.error_percent);
    let text = report.to_string();
    assert!(text.contains("FAIL") && text.ends_with("Overall: FAIL"));

    // The same result passes once the tolerance is widened: tolerances are
    // data, not code.
    set.targets[0].tolerance_percent = 200.0;
    assert!(validate(&root, &set).unwrap().passed());
}

#[test]
fn a_track_without_a_skidpad_is_reported_not_guessed() {
    let root = data();
    let mut set = TargetSet::load(&root, "hatch_tern_gt").unwrap();
    set.track = "gravel_stage".into();
    set.targets.retain(|t| t.metric == Metric::SkidpadLateralG);
    let report = validate(&root, &set).unwrap();
    assert!(!report.passed());
    assert_eq!(report.failures.len(), 1);
    assert!(report.failures[0].1.contains("circle marker"));
}
