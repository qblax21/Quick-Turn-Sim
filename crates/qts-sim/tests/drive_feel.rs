// SPDX-License-Identifier: Apache-2.0
//! Regressions from a drive (D-088): pulling away on lock, the electric
//! sedan's lift-off snap, and the steering column's torsion bar.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::step::Simulation;
use qts_sim::drivers::{Driver, PathFollower, PathFollowerConfig};
use qts_sim::session::Session;

#[test]
fn a_car_pulls_away_with_the_wheel_turned() {
    // Traction control used to read a steered front wheel's longer path
    // as wheelspin and cut the engine to almost nothing.
    for car in CARS {
        for steer in [0.0, 0.6, 1.0] {
            let mut s = session(car, "test_track");
            let frame = InputFrame::from_analog(steer, 0.35, 0.0, 0.0, 0.0);
            for _ in 0..4000 {
                s.step(&frame);
            }
            let v = s.car().output().speed;
            assert!(v > 5.0, "{car} at steer {steer}: only {v:.2} m/s after 4 s");
        }
    }
}

#[test]
fn the_electric_sedan_does_not_snap_on_a_lift_at_speed() {
    // A quick turn-in at 100 km/h and a lift: the rear used to slide to 16
    // degrees; with the roll stiffness moved forward it stays near 7.
    let mut s = session("sedan_halden_e4", "test_track");
    let max = s.car().params().steering.max_wheel_angle_rad;
    let (x, y, h) = s.track().spawn();
    s.car_mut().place(x, y, h, 0.0);
    s.car_mut().set_forward_speed(28.0);
    for _ in 0..1500 {
        s.step(&InputFrame::from_analog(0.0, 0.4, 0.0, 0.0, 0.0));
    }
    let mut angle = 0.0_f64;
    let mut worst: f64 = 0.0;
    for t in 0..3000 {
        angle = (angle + 8.0 * 0.001).min(40_f64.to_radians());
        let throttle = if t > 600 { 0.0 } else { 0.4 };
        s.step(&InputFrame::from_analog(
            angle / max,
            throttle,
            0.0,
            0.0,
            0.0,
        ));
        let v = s.car().output().vel_body;
        worst = worst.max((v[1] / v[0].max(0.5)).atan().to_degrees().abs());
    }
    assert!(worst < 10.0, "body slip reached {worst:.1} degrees");
}

/// A scripted Ridgeline lap, hand-limited steering; returns per tick the
/// torque on the rack side of the torsion bar and the torque felt, N·m.
fn lap(car: &str) -> Vec<(f64, f64, f64)> {
    let mut s: Session = session(car, "ridgeline_circuit");
    let path = s.track().path("circuit").unwrap().clone();
    let mut d = PathFollower::new(
        &path,
        PathFollowerConfig {
            max_speed: 40.0,
            lateral_accel: 7.0,
            closed: true,
            ..PathFollowerConfig::default()
        },
    );
    let max = s.car().params().steering.max_wheel_angle_rad;
    let full = f64::from(qts_core::input::AXIS_MAX);
    let (mut angle, mut rate) = (0.0_f64, 0.0_f64);
    let mut out = Vec::new();
    for _ in 0..60_000 {
        let mut f = d.frame(&s);
        let target = f64::from(f.steer) / full * max;
        let want = ((target - angle) / 0.04).clamp(-12.0, 12.0);
        rate += (want - rate).clamp(-0.3, 0.3);
        angle += rate * 0.001;
        f.steer = ((angle / max).clamp(-1.0, 1.0) * full).round() as i16;
        s.step(&f);
        let o = s.car().output();
        let st = o.steering;
        let rack = st.load_nm + st.assist_nm + st.friction_nm + st.inertia_nm;
        out.push((rack, o.steering_torque_nm, o.accel_body[1]));
    }
    out
}

#[test]
fn the_torsion_bar_passes_steady_load_and_softens_shocks() {
    for car in ["hatch_tern_gt", "sedan_halden_e4"] {
        let rows = lap(car);
        // High-frequency content: the tick-to-tick change, RMS.
        let hf = |k: usize| {
            let sum: f64 = rows
                .windows(2)
                .map(|w| {
                    let (a, b) = if k == 0 {
                        (w[0].0, w[1].0)
                    } else {
                        (w[0].1, w[1].1)
                    };
                    (b - a).powi(2)
                })
                .sum();
            (sum / rows.len() as f64).sqrt()
        };
        let (rack_hf, felt_hf) = (hf(0), hf(1));
        assert!(
            felt_hf < 0.35 * rack_hf,
            "{car}: felt {felt_hf:.4} against rack side {rack_hf:.4} N.m per tick"
        );
        // Steady cornering on the skidpad circle: the bar passes the load
        // unchanged.
        let mut s = session(car, "test_track");
        let (c, r) = s
            .track()
            .def()
            .markers
            .iter()
            .find_map(|m| match m {
                qts_sim::track::MarkerDef::Circle {
                    center_x,
                    center_y,
                    radius_m,
                    ..
                } => Some(([*center_x, *center_y], *radius_m)),
                _ => None,
            })
            .unwrap();
        let start = [c[0], c[1] - r];
        s.car_mut().place(start[0], start[1], 0.0, 0.0);
        s.car_mut().set_forward_speed(14.0);
        let mut d = PathFollower::circle(
            c,
            r,
            start,
            PathFollowerConfig {
                max_speed: 14.0,
                lateral_accel: 60.0,
                closed: true,
                ..PathFollowerConfig::default()
            },
        );
        let (mut rack, mut felt) = (0.0, 0.0);
        for t in 0..15_000 {
            let f = d.frame(&s);
            s.step(&f);
            if t >= 13_000 {
                let st = s.car().output().steering;
                rack += st.load_nm + st.assist_nm + st.friction_nm + st.inertia_nm;
                felt += s.car().output().steering_torque_nm;
            }
        }
        assert!(rack.abs() > 1000.0, "{car}: no load on the circle");
        assert!(
            (felt - rack).abs() < 0.01 * rack.abs(),
            "{car}: steady corner felt {:.3} against {:.3} N.m",
            felt / 2000.0,
            rack / 2000.0
        );
    }
}
