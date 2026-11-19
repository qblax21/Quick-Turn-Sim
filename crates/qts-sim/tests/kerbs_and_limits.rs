// SPDX-License-Identifier: Apache-2.0
//! Kerb ramps tilt the ground under the tire, so a kerb pushes sideways
//! as well as up; and a lap with all four wheels beyond the road and kerbs
//! does not count.

mod common;

use common::*;
use qts_core::step::Simulation;
use qts_sim::drivers::{Driver, PathFollower, PathFollowerConfig};
use qts_sim::{Session, SessionAssets};
use qts_vehicle::Ground;

/// A point `across` metres beyond the inner edge of Ridgeline's first kerb
/// (right of the path, 6 m out, at 500 m along the lap), and the unit
/// vector pointing from it back toward the path.
fn on_first_kerb(s: &Session, across: f64) -> ([f64; 2], [f64; 2]) {
    let path = s.track().path("circuit").unwrap();
    let (p, heading) = path.at(500.0);
    let right = [heading.sin(), -heading.cos()];
    let out = 6.0 + across;
    (
        [p[0] + right[0] * out, p[1] + right[1] * out],
        [-right[0], -right[1]],
    )
}

#[test]
fn kerb_ramps_tilt_the_ground_toward_the_road() {
    let s = session("hatch_tern_gt", "ridgeline_circuit");
    let track = s.track();
    let tilt_toward_road = |across: f64| {
        let (p, inward) = on_first_kerb(&s, across);
        let n = track.sample(p[0], p[1]).normal;
        let base = track.ground_normal(p[0], p[1]);
        // Horizontal tilt added by the kerb, as a slope, toward the road.
        (
            (n.x / n.z - base.x / base.z) * inward[0] + (n.y / n.z - base.y / base.z) * inward[1],
            n,
        )
    };
    // On the inner ramp (40 mm over the first 0.36 m) the ground leans
    // back toward the road by the ramp's slope.
    let (ramp, _) = tilt_toward_road(0.15);
    assert!((ramp - 0.04 / 0.36).abs() < 0.005, "ramp slope {ramp:.4}");
    // On the flat top it does not lean.
    let (top, _) = tilt_toward_road(0.7);
    assert!(top.abs() < 1e-9, "flat top slope {top}");
    // On the outer ramp it leans away from the road.
    let (outer, _) = tilt_toward_road(1.12);
    assert!(outer < -0.1, "outer ramp slope {outer:.4}");
    // Half a tire inside the edge, on the road, the tread already rests
    // partly on the ramp.
    let (edge, _) = tilt_toward_road(-0.05);
    assert!(edge > 0.0 && edge < ramp, "edge slope {edge:.4}");
    // Well inside, nothing.
    let (road, n) = tilt_toward_road(-0.5);
    assert!(road.abs() < 1e-9);
    assert!((n.length() - 1.0).abs() < 1e-12);
}

#[test]
fn riding_a_kerb_drives_differently_with_tilted_normals() {
    // Right wheels on the first kerb through the T1 right-hander: the car's
    // centre 5.6 m right of the path puts the right wheels at about 6.35 m.
    let run = |tilt: bool| {
        let mut assets =
            SessionAssets::load(&data(), "hatch_tern_gt", "ridgeline_circuit").unwrap();
        assets.track.set_legacy_kerbs(!tilt);
        let mut s = Session::new(
            assets,
            *session("hatch_tern_gt", "ridgeline_circuit").config(),
        )
        .unwrap();
        let path = s.track().path("circuit").unwrap().clone();
        let mut driver = PathFollower::offset(
            &path,
            -5.6,
            PathFollowerConfig {
                max_speed: 18.0,
                ..PathFollowerConfig::default()
            },
        );
        let kerb = s
            .track()
            .surfaces()
            .iter()
            .position(|x| x.id == "kerb")
            .unwrap() as u16;
        let mut kerb_ticks = 0;
        for _ in 0..60_000 {
            let frame = driver.frame(&s);
            s.step(&frame);
            if s.car().output().wheels[1].surface_index == kerb {
                kerb_ticks += 1;
            }
            if driver.progress() > 560.0 {
                break;
            }
        }
        (kerb_ticks, qts_core::snapshot::state_hash(&s))
    };
    let (ticks_flat, hash_flat) = run(false);
    let (ticks_tilt, hash_tilt) = run(true);
    assert!(
        ticks_flat > 500 && ticks_tilt > 500,
        "{ticks_flat} {ticks_tilt} ticks on the kerb"
    );
    assert_ne!(
        hash_flat, hash_tilt,
        "tilting the kerb normal must change the drive"
    );
}

#[test]
fn kerbs_taper_to_nothing_at_their_ends() {
    let s = session("hatch_tern_gt", "ridgeline_circuit");
    let path = s.track().path("circuit").unwrap();
    // The first kerb runs from 468.8 m to 525.4 m, 6.0 to 7.2 m right of
    // the path; 0.7 m across is on its flat top.
    let height_at = |along: f64| {
        let (p, h) = path.at(along);
        let out = 6.7;
        let (x, y) = (p[0] + h.sin() * out, p[1] - h.cos() * out);
        s.track().surface_at(x, y).1
    };
    assert!(height_at(468.9) < 0.008, "{}", height_at(468.9));
    assert!(
        (height_at(469.3) - 0.02).abs() < 0.003,
        "{}",
        height_at(469.3)
    );
    assert!((height_at(500.0) - 0.04).abs() < 1e-9);
    assert!(height_at(525.3) < 0.008);
}

/// Follows the circuit, `offset` metres to the left of it between
/// `from_m` and `to_m` along the lap and on it elsewhere.
fn detour(s: &Session, offset: f64, from_m: f64, to_m: f64) -> PathFollower {
    let path = s.track().path("circuit").unwrap();
    let points = path
        .points
        .iter()
        .zip(&path.s)
        .zip(&path.heading)
        .map(|((p, d), h)| {
            let blend = if *d < from_m - 40.0 || *d > to_m + 40.0 {
                0.0
            } else if *d < from_m {
                (*d - from_m + 40.0) / 40.0
            } else if *d > to_m {
                (to_m + 40.0 - *d) / 40.0
            } else {
                1.0
            };
            let o = offset * blend;
            [p[0] - h.sin() * o, p[1] + h.cos() * o]
        })
        .collect();
    PathFollower::through(
        points,
        PathFollowerConfig {
            max_speed: 24.0,
            lateral_accel: 6.0,
            closed: true,
            ..PathFollowerConfig::default()
        },
    )
}

#[test]
fn a_lap_with_four_wheels_off_does_not_count_and_one_with_two_does() {
    // Up the start straight 8.5 m left of centre: every wheel is beyond the
    // 6 m road (there is no kerb there).
    let mut s = session("hatch_tern_gt", "ridgeline_circuit");
    let mut driver = detour(&s, 8.5, 150.0, 300.0);
    let mut warned_at = None;
    for _ in 0..400_000 {
        let frame = driver.frame(&s);
        s.step(&frame);
        if warned_at.is_none() && s.timing().limits_broken {
            warned_at = Some(driver.progress());
        }
        if s.timing().runs + s.timing().invalid_runs > 0 && s.timing().running {
            break;
        }
    }
    let at = warned_at.expect("the excursion must break the limits");
    // All four wheels are past 6 m once the centre is 6.75 m out, part way
    // through the 40 m ease-in before 150 m.
    assert!((110.0..300.0).contains(&at), "broken at {at:.0} m");
    assert_eq!(s.timing().runs, 0, "the lap must not count");
    assert_eq!(s.timing().invalid_runs, 1);
    assert_eq!(s.timing().limit_breaches, 1);
    // The next lap starts clean.
    assert!(!s.timing().limits_broken);

    // Two wheels over the edge (centre 6.3 m out on a 1.5 m track) is
    // still on the track.
    let mut s = session("hatch_tern_gt", "ridgeline_circuit");
    let mut driver = detour(&s, 6.3, 150.0, 300.0);
    for _ in 0..400_000 {
        let frame = driver.frame(&s);
        s.step(&frame);
        if s.timing().runs + s.timing().invalid_runs > 0 {
            break;
        }
    }
    assert_eq!(s.timing().limit_breaches, 0);
    assert_eq!(
        s.timing().runs,
        1,
        "invalid runs: {}",
        s.timing().invalid_runs
    );
}

#[test]
fn tracks_without_limits_never_break_them() {
    let s = session("hatch_tern_gt", "autocross_lot");
    assert!(!s.track().has_limits());
    assert!(s.track().within_limits(1.0e4, 1.0e4));
    let r = session("hatch_tern_gt", "ridgeline_circuit");
    assert!(r.track().has_limits());
    let (on_kerb, _) = on_first_kerb(&r, 0.6);
    assert!(
        r.track().within_limits(on_kerb[0], on_kerb[1]),
        "a kerb is track"
    );
    let (beyond, _) = on_first_kerb(&r, 1.5);
    assert!(!r.track().within_limits(beyond[0], beyond[1]));
}
