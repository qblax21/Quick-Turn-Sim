// SPDX-License-Identifier: Apache-2.0
//! Tire temperature against published figures and on the first lap from
//! the pits (D-108, D-109). Since D-109 grip and cornering stiffness fall
//! from cold to hot as Goodyear measured on one high-performance summer
//! tire; the two published comparisons are ordinary tests again.

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
fn out_lap_grip_follows_the_published_direction() {
    // The hatch from the pits on Ridgeline with the tires at the air
    // temperature (20 C) and already hot (45 C), driven close to the limit
    // (the path follower planning corners for 8 m/s2). Since D-109 grip is
    // highest cool and falls as the tire heats, as Goodyear measured.
    let root = data();
    let cool = drive(&root, "hatch_tern_gt", "ridgeline_circuit", 20.0, 8.0, 1).unwrap();
    let hot = drive(&root, "hatch_tern_gt", "ridgeline_circuit", 45.0, 8.0, 1).unwrap();
    let w = warm_up(&cool[0], &hot[0], 0.005).unwrap();
    eprintln!(
        "lap from the pits: {:.3} s from 20 C, {:.3} s from 45 C; grip at T1 {:.4} and {:.4}",
        cool[0].lap_s, hot[0].lap_s, w.grip[0].0, w.grip[0].1
    );
    // Cool tires grip better than hot ones at the first corner.
    assert!(w.grip[0].0 > w.grip[0].1 + 0.005, "{:?}", w.grip[0]);
    // The cool tires heat through the lap...
    let first = w.cold_temps[0];
    let last = *w.cold_temps.last().unwrap();
    assert!(last[1] > first[1] + 5.0, "core {first:?} -> {last:?}");
    assert!(last[2] > first[2] + 3.0, "carcass {first:?} -> {last:?}");
    // ...and lose grip where they are hottest.
    let (hottest, _) = w
        .cold_temps
        .iter()
        .enumerate()
        .max_by(|a, b| a.1[0].total_cmp(&b.1[0]))
        .unwrap();
    assert!(
        w.grip[hottest].0 < w.grip[0].0 - 0.005,
        "grip {:.4} at the hottest corner against {:.4} at the first",
        w.grip[hottest].0,
        w.grip[0].0
    );
}

/// Published: a high-performance summer tire on a Flat-Trac machine loses
/// about 10 % of its peak grip going from cold to hot (about 30 to 70 C
/// surface). Singh and Sivaramakrishnan (Goodyear), arXiv:2305.18422,
/// Table 3 and Figure 26.
#[test]
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
/// tied to the bulk temperature (same source).
#[test]
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
