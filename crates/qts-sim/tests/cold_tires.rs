// SPDX-License-Identifier: Apache-2.0
//! Cold tires on the first lap from the pits (D-108). No physics was
//! changed for this: the tests hold the model's present behaviour, and the
//! two places where it differs from published measurements are kept as
//! ignored tests so the gap stays visible until it is decided on
//! (`cargo test -- --ignored` runs them).

mod common;

use common::*;
use qts_sim::cold_start::{
    change, drive, factors, warm_up, PUBLISHED_GRIP_CHANGE, PUBLISHED_HOT_C,
    PUBLISHED_STIFFNESS_CHANGE,
};

#[test]
fn grip_and_stiffness_against_temperature_are_sane_for_every_tire() {
    let root = data();
    for id in root.list("tires").unwrap() {
        let p = root.load_tire(&id).unwrap().value;
        let f = factors(&p, 0.0, 120.0, 1.0);
        let peak = f.iter().map(|x| x.1).fold(0.0, f64::max);
        let at_peak = f.iter().find(|x| x.1 == peak).unwrap().0;
        assert!(
            (peak - 1.0).abs() < 1e-3,
            "{id}: grip factor peaks at {peak}"
        );
        assert!(
            (p.thermal.optimal_temp_c - at_peak).abs() <= 1.0,
            "{id}: peak at {at_peak} C"
        );
        for w in f.windows(2) {
            // Grip within its floor and one; rising to the optimum and
            // falling after it; stiffness falling as the carcass warms.
            assert!(w[1].1 >= p.thermal.min_grip_factor && w[1].1 <= 1.0 + 1e-9);
            if w[1].0 <= p.thermal.optimal_temp_c {
                assert!(w[1].1 >= w[0].1 - 1e-12, "{id} at {} C", w[1].0);
            } else {
                assert!(w[1].1 <= w[0].1 + 1e-12, "{id} at {} C", w[1].0);
            }
            assert!(w[1].2 <= w[0].2 + 1e-12);
        }
    }
}

#[test]
fn cold_tires_cost_grip_at_first_and_warm_up_within_the_lap() {
    // The hatch from the pits on Ridgeline with the tires at the air
    // temperature (20 C) and at their optimum (45 C), driven close to the
    // limit (the path follower planning corners for 8 m/s2).
    let root = data();
    let cold = drive(&root, "hatch_tern_gt", "ridgeline_circuit", 20.0, 8.0, 1).unwrap();
    let warm = drive(&root, "hatch_tern_gt", "ridgeline_circuit", 45.0, 8.0, 1).unwrap();
    let w = warm_up(&cold[0], &warm[0], 0.005).unwrap();
    eprintln!(
        "lap from the pits: {:.3} s cold, {:.3} s warm; grip at T1 {:.4} cold, {:.4} warm; within 0.5 % from corner {:?}",
        cold[0].lap_s,
        warm[0].lap_s,
        w.grip[0].0,
        w.grip[0].1,
        w.warm_after.map(|n| n + 1)
    );
    // Measurably less grip at the first corner.
    assert!(w.grip[0].0 < w.grip[0].1 - 0.005, "{:?}", w.grip[0]);
    // The tires warm through the lap...
    let first = w.cold_temps[0];
    let last = *w.cold_temps.last().unwrap();
    assert!(last[1] > first[1] + 5.0, "core {first:?} -> {last:?}");
    assert!(last[2] > first[2] + 3.0, "carcass {first:?} -> {last:?}");
    // ...and the grip is within half a percent of the warm tires' from a
    // few corners in to the end of the lap.
    let n = w
        .warm_after
        .expect("cold tires never caught up within the lap");
    assert!((2..=8).contains(&n), "warm after {n} corners");
    // The first lap is not faster on cold tires.
    assert!(cold[0].lap_s >= warm[0].lap_s - 0.05);
}

/// Published: a high-performance summer tire on a Flat-Trac machine loses
/// about 10 % of its peak grip going from cold to hot (about 30 to 70 C
/// surface). Singh and Sivaramakrishnan (Goodyear), arXiv:2305.18422,
/// Table 3 and Figure 26. The model loses 1.5 %, and gains grip between
/// 30 and its 45 C optimum.
#[test]
#[ignore = "known gap (D-108): the model's grip changes far less with temperature than published; not changed pending a decision"]
fn grip_falls_from_cold_to_hot_as_published() {
    let root = data();
    for id in root.list("tires").unwrap() {
        let p = root.load_tire(&id).unwrap().value;
        let (g, _) = change(&p, PUBLISHED_HOT_C.0, PUBLISHED_HOT_C.1);
        assert!(
            (g - PUBLISHED_GRIP_CHANGE).abs() <= 0.05,
            "{id}: grip changes {:+.1} % from 30 to 70 C, published about {:+.0} %",
            100.0 * g,
            100.0 * PUBLISHED_GRIP_CHANGE
        );
    }
}

/// Published: cornering stiffness 20 to 25 % lower hot than cold, strongly
/// tied to the bulk temperature (same source). The model's stiffness factor
/// falls 7.8 % from 30 to 70 C.
#[test]
#[ignore = "known gap (D-108): the model's cornering stiffness changes about a third as much with temperature as published"]
fn stiffness_falls_from_cold_to_hot_as_published() {
    let root = data();
    for id in root.list("tires").unwrap() {
        let p = root.load_tire(&id).unwrap().value;
        let (_, k) = change(&p, PUBLISHED_HOT_C.0, PUBLISHED_HOT_C.1);
        assert!(
            k >= PUBLISHED_STIFFNESS_CHANGE.0 - 0.02 && k <= PUBLISHED_STIFFNESS_CHANGE.1 + 0.02,
            "{id}: stiffness changes {:+.1} % from 30 to 70 C, published {:.0} to {:.0} %",
            100.0 * k,
            100.0 * PUBLISHED_STIFFNESS_CHANGE.0,
            100.0 * PUBLISHED_STIFFNESS_CHANGE.1
        );
    }
}
