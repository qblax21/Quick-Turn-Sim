// SPDX-License-Identifier: Apache-2.0
//! Drift judging (D-114): the scoring on line, angle and style with its
//! penalties, the zones against the painted clipping points, and the live
//! judge on a lap of Harrow Drift Park.

mod common;

use common::*;
use qts_core::step::Simulation;
use qts_sim::drift::{path_point, score_zone, DriftJudge, Penalty, Sample};
use qts_sim::drivers::{Driver, PathFollower, PathFollowerConfig};
use qts_sim::track::{DriftZoneDef, MarkerDef};

const DT: f64 = 0.01;

/// A right-hander from 0 to 100 m: ICP on the right at 50 m, OCZ on the
/// left from 25 to 75 m.
fn zone() -> DriftZoneDef {
    DriftZoneDef {
        name: "T".into(),
        from_m: 0.0,
        to_m: 100.0,
        icp_at_m: 50.0,
        icp_offset_m: -7.6,
        ocz_from_m: 25.0,
        ocz_to_m: 75.0,
        ocz_offset_m: 6.0,
    }
}

/// Samples every 10 cm along the zone from a function of distance.
fn run(f: impl Fn(f64) -> Sample) -> Vec<Sample> {
    (0..=1000).map(|i| f(f64::from(i) * 0.1)).collect()
}

fn sample(s: f64, slip: f64, gap: f64, tail: f64) -> Sample {
    Sample {
        s,
        slip_deg: slip,
        icp_gap_m: gap,
        tail_offset_m: tail,
        on_course: true,
    }
}

#[test]
fn a_committed_slide_on_both_clipping_points_scores_full_marks() {
    let z = zone();
    let r = score_zone(
        &z,
        &run(|s| sample(s, 40.0, 0.4 + (s - 50.0).abs() * 0.1, 6.2)),
        DT,
    );
    assert_eq!(r.penalty, None);
    assert!((r.total - 100.0).abs() < 1e-9, "{r:?}");
    assert!(r.ocz_short_m <= 0.0);
}

#[test]
fn driving_round_on_grip_scores_little() {
    let z = zone();
    let r = score_zone(&z, &run(|s| sample(s, 2.0, 3.0, 2.0)), DT);
    assert_eq!(r.angle, 0.0);
    assert_eq!(r.style, 0.0);
    // 3 m from the ICP: 0.29 of its half; 4 m short of the OCZ: nothing.
    assert!((r.line - 0.5 * (1.0 - 2.5 / 3.5)).abs() < 1e-9, "{r:?}");
    assert!(r.total < 6.0, "{r:?}");
}

#[test]
fn angle_and_line_are_marked_on_a_sliding_scale() {
    let z = zone();
    let half_angle = score_zone(&z, &run(|s| sample(s, 20.0, 0.4, 6.2)), DT);
    assert!((half_angle.angle - 0.5).abs() < 1e-9, "{half_angle:?}");
    let short = score_zone(&z, &run(|s| sample(s, 40.0, 0.4, 4.0)), DT);
    assert!((short.ocz_short_m - 2.0).abs() < 1e-9);
    assert!((short.line - 0.5 * (1.0 + 0.5)).abs() < 1e-9, "{short:?}");
}

#[test]
fn straightening_mid_corner_costs_style() {
    let z = zone();
    let r = score_zone(
        &z,
        &run(|s| {
            sample(
                s,
                if (40.0..50.0).contains(&s) { 2.0 } else { 40.0 },
                0.4,
                6.2,
            )
        }),
        DT,
    );
    assert_eq!(r.straightened, 1);
    assert!((r.style - (0.9 - 0.3)).abs() < 0.01, "{r:?}");
}

#[test]
fn a_spin_or_running_off_zeroes_the_corner() {
    let z = zone();
    let spun = score_zone(
        &z,
        &run(|s| sample(s, if s > 60.0 { 130.0 } else { 40.0 }, 0.4, 6.2)),
        DT,
    );
    assert_eq!(spun.penalty, Some(Penalty::Spin));
    assert_eq!(spun.total, 0.0);
    // 70 samples off at 0.01 s: 0.7 s, over the half second allowed.
    let off = score_zone(
        &z,
        &run(|s| Sample {
            on_course: !(60.0..67.0).contains(&s),
            ..sample(s, 40.0, 0.4, 6.2)
        }),
        DT,
    );
    assert_eq!(off.penalty, Some(Penalty::OffCourse));
    // A brief excursion is allowed.
    let brief = score_zone(
        &z,
        &run(|s| Sample {
            on_course: !(60.0..62.0).contains(&s),
            ..sample(s, 40.0, 0.4, 6.2)
        }),
        DT,
    );
    assert_eq!(brief.penalty, None);
}

#[test]
fn harrow_zones_match_the_painted_clipping_points() {
    let s = session("coupe_kestrel_rc", "harrow_drift_park");
    let def = s.track().def().drift.clone().expect("drift zones");
    let path = s.track().path(&def.path).unwrap();
    assert_eq!(def.zones.len(), 9);
    let circles: Vec<(f64, f64, String)> = s
        .track()
        .def()
        .markers
        .iter()
        .filter_map(|m| match m {
            MarkerDef::Circle {
                center_x,
                center_y,
                label,
                ..
            } => Some((*center_x, *center_y, label.clone())),
            MarkerDef::Line { .. } => None,
        })
        .collect();
    for (z, w) in def.zones.iter().zip(def.zones.iter().skip(1)) {
        assert!(
            z.to_m <= w.from_m + 1e-9,
            "zones overlap: {} and {}",
            z.name,
            w.name
        );
    }
    for z in &def.zones {
        let p = path_point(path, z.icp_at_m, z.icp_offset_m);
        let (x, y, _) = circles
            .iter()
            .find(|c| c.2 == format!("ICP {}", z.name))
            .unwrap_or_else(|| panic!("no painted ICP for {}", z.name));
        assert!(
            (p[0] - x).hypot(p[1] - y) < 0.05,
            "{}: judged ICP off the paint",
            z.name
        );
        assert!(z.from_m < z.icp_at_m && z.icp_at_m < z.to_m);
        assert!(z.ocz_from_m >= z.from_m && z.ocz_to_m <= z.to_m);
        // ICP on the inside, OCZ on the outside.
        assert!(z.icp_offset_m * z.ocz_offset_m < 0.0, "{}", z.name);
    }
}

#[test]
fn the_live_judge_scores_every_corner_of_a_lap() {
    // The path follower drives the centre line on grip: the judge must see
    // every corner, place the car about 6.8 m from each inner clipping
    // point (7.6 m less half the car's width), and give no angle marks.
    let mut s = session("coupe_kestrel_rc", "harrow_drift_park");
    let path = s.track().path("circuit").unwrap().clone();
    let mut driver = PathFollower::new(
        &path,
        PathFollowerConfig {
            closed: true,
            ..PathFollowerConfig::default()
        },
    );
    let mut judge = DriftJudge::new(&s);
    let slot = judge.slot();
    for _ in 0..400_000 {
        let frame = driver.frame(&s);
        s.step(&frame);
        judge.observe(&s);
        if s.timing().runs >= 1 {
            break;
        }
    }
    let got = slot.lock().unwrap().clone();
    assert!(got.active);
    assert_eq!(got.serial, 1);
    let lap = got.last_lap.expect("a judged lap");
    assert!(lap.counted);
    assert_eq!(lap.zones.len(), 9);
    for z in &lap.zones {
        let z = z.as_ref().expect("a corner was not judged");
        eprintln!("{} {:5.1}  {}", z.name, z.total, z.describe());
        assert_eq!(z.penalty, None, "{}", z.name);
        assert!(
            (5.5..8.0).contains(&z.icp_gap_m),
            "{}: ICP gap {:.2} m",
            z.name,
            z.icp_gap_m
        );
        assert!(z.angle < 0.2, "{}: angle marks on grip", z.name);
        assert!(z.total < 30.0, "{}: {:.1}", z.name, z.total);
    }
    eprintln!("lap {:.1}", lap.total);
    // On a track without zones the judge does nothing.
    let plain = DriftJudge::new(&session("coupe_kestrel_rc", "test_track"));
    assert!(!plain.slot().lock().unwrap().active);
}

#[test]
fn the_judge_reads_a_real_slide() {
    // A power slide on Harrow's start straight, past the start gate: the
    // judge's live angle follows the car's body slip.
    use qts_core::input::InputFrame;
    let mut s = session("coupe_kestrel_rc", "harrow_drift_park");
    let (x, y, h) = s.track().spawn();
    s.car_mut().place(x, y, h, 0.0);
    s.car_mut().set_forward_speed(14.0);
    let mut judge = DriftJudge::new(&s);
    let slot = judge.slot();
    let p = s.car().params().steering.clone();
    let turn = 15_f64.to_radians() * p.ratio / p.max_wheel_angle_rad;
    let mut peak: f64 = 0.0;
    for t in 0..3500 {
        let (steer, throttle) = if t < 2500 { (0.0, 0.3) } else { (turn, 1.0) };
        let mut f = InputFrame::from_analog(steer, throttle, 0.0, 0.0, 0.0);
        f.gear_request = 2;
        s.step(&f);
        judge.observe(&s);
        let v = s.car().output().vel_body;
        let slip = v[1].atan2(v[0]).to_degrees().abs();
        if t % 10 == 9 && s.timing().running {
            let read = slot.lock().unwrap().angle_deg;
            assert!(
                (read - slip).abs() < 3.0,
                "judge reads {read:.1}, car at {slip:.1}"
            );
            peak = peak.max(read);
        }
    }
    assert!(peak > 15.0, "the slide never showed: {peak:.1} deg");
}
