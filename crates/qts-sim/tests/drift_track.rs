// SPDX-License-Identifier: Apache-2.0
//! Harrow Drift Park (D-113): the lap closes and can be driven and timed
//! in the drift car, and its painted clipping points sit where drift
//! judging puts them: one inner clipping point at the inside edge of each
//! corner's apex, and an outer clipping zone along the outside.

mod common;

use common::*;
use qts_core::step::Simulation;
use qts_sim::drivers::{Driver, PathFollower, PathFollowerConfig};
use qts_sim::track::MarkerDef;

const TRACK: &str = "harrow_drift_park";
/// Road half width, m.
const HALF: f64 = 7.0;

#[test]
fn clipping_points_mark_every_corner() {
    let s = session("coupe_kestrel_rc", TRACK);
    let track = s.track();
    let path = track.path("circuit").unwrap();
    let mut inner = 0;
    let mut outer = 0;
    for m in &track.def().markers {
        match m {
            MarkerDef::Circle {
                center_x,
                center_y,
                label,
                ..
            } => {
                assert!(label.starts_with("ICP "), "{label}");
                inner += 1;
                // Just inside the road's inner edge.
                let (_, off) = path
                    .project(*center_x, *center_y, 0.0, path.length(), 20.0)
                    .expect("clipping point off the course");
                assert!(
                    (off.abs() - (HALF + 0.6)).abs() < 0.3,
                    "{label} is {:.2} m from the centre line",
                    off.abs()
                );
            }
            MarkerDef::Line { x1, y1, label, .. } => {
                assert!(label.starts_with("OCZ "), "{label}");
                outer += 1;
                let (_, off) = path
                    .project(*x1, *y1, 0.0, path.length(), 20.0)
                    .expect("clipping zone off the course");
                assert!(
                    (off.abs() - (HALF - 1.0)).abs() < 0.3,
                    "{label} at {off:.2} m"
                );
            }
        }
    }
    // Nine corners: one inner point and three outer zone lines each.
    assert_eq!(inner, 9);
    assert_eq!(outer, 27);
}

#[test]
fn the_drift_car_laps_it_and_the_lap_counts() {
    let mut s = session("coupe_kestrel_rc", TRACK);
    let path = s.track().path("circuit").unwrap().clone();
    let mut driver = PathFollower::new(
        &path,
        PathFollowerConfig {
            closed: true,
            ..PathFollowerConfig::default()
        },
    );
    for _ in 0..400_000 {
        let frame = driver.frame(&s);
        s.step(&frame);
        if s.timing().runs >= 1 {
            break;
        }
    }
    let t = s.timing();
    assert_eq!(
        t.runs, 1,
        "no counted lap (invalid laps: {})",
        t.invalid_runs
    );
    let lap = t.last_ticks as f64 / f64::from(s.tick_rate_hz());
    eprintln!("Kestrel RC, path follower: lap {lap:.2} s");
    assert!(lap > 60.0 && lap < 200.0, "{lap} s");
}
