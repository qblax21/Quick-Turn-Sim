// SPDX-License-Identifier: Apache-2.0
//! Force feedback in a drift (D-116): on the Kestrel RC with its angle kit,
//! a wheel let go of in a slide is turned into opposite lock by the
//! self-aligning torque alone, and settles at the soft lock instead of
//! bouncing off it.

mod common;

use common::*;
use qts_core::input::{InputFrame, AXIS_MAX};
use qts_core::step::Simulation;
use qts_ffb::{matched_steer, FfbPipeline};
use qts_sim::host::ffb_input;
use qts_sim::session::{Session, SessionConfig};
use qts_vehicle::Assists;

const CAR: &str = "coupe_kestrel_rc";

fn body_slip_deg(s: &Session) -> f64 {
    let v = s.car().output().vel_body;
    v[1].atan2(v[0].abs().max(0.5)).to_degrees()
}

/// One hands-off run. Returns per-ms (time s, rim angle as a fraction of
/// the car's lock, body slip deg, rim rate rad/s).
fn hands_off(rim_inertia: f64) -> Vec<(f64, f64, f64, f64)> {
    let config = SessionConfig {
        assists: Assists {
            auto_clutch: true,
            auto_shift: false,
            traction_control: false,
            engine_damage: false,
        },
        ..SessionConfig::default()
    };
    let mut s = Session::from_data(&data(), CAR, "test_track", config).unwrap();
    let (x, y, h) = s.track().spawn();
    s.car_mut().place(x, y, h, 0.0);
    s.car_mut().set_forward_speed(14.0);
    let lock = s.car().params().steering.max_wheel_angle_rad;
    let ratio = s.car().params().steering.ratio;
    let mut cfg = data().load_ffb("fanatec_csl_elite").unwrap().value;
    // The base set to its full 1080 degrees, so the car's lock (about 520
    // degrees each way) is inside it and the soft lock is the wall.
    let rotation = 1080_f64.to_radians();
    cfg.device_rotation_rad = rotation;
    let peak = cfg.device_max_torque_nm;
    let mut pipeline = FfbPipeline::new(cfg.clone(), s.tick_rate_hz()).unwrap();
    let frame = |steer: f64, throttle: f64| {
        let mut f = InputFrame::from_analog(0.0, throttle, 0.0, 0.0, 0.0);
        f.steer = (steer.clamp(-1.0, 1.0) * f64::from(AXIS_MAX)).round() as i16;
        f.gear_request = 2;
        f
    };
    for _ in 0..800 {
        s.step(&frame(0.0, 0.2));
    }
    // Turn in left at 15 degrees of road wheel and floor it: the rear
    // comes round. The rim follows the driver's hands until let go.
    let turn_in = 15_f64.to_radians() * ratio;
    for _ in 0..500 {
        s.step(&frame(turn_in / lock, 1.0));
        pipeline.step(&ffb_input(&s, Some(turn_in)));
    }
    let step = rotation / 65536.0;
    let (mut angle, mut rate) = (turn_in, 0.0_f64);
    let mut late = std::collections::VecDeque::from(vec![0.0; 4]);
    let mut out = Vec::new();
    for t in 0..3000 {
        let read = (angle / step).round() * step;
        let slip = body_slip_deg(&s);
        let throttle = (0.6 + 0.03 * (25.0 - slip.abs())).clamp(0.1, 1.0);
        s.step(&frame(
            matched_steer(read / (0.5 * rotation), rotation, lock),
            throttle,
        ));
        let sample = pipeline.step(&ffb_input(&s, Some(read)));
        late.push_back(f64::from(sample.device_command(&cfg)) * peak);
        rate += late.pop_front().unwrap() / rim_inertia * 0.001;
        angle += rate * 0.001;
        out.push((f64::from(t) * 0.001, angle / lock, slip, rate));
    }
    out
}

struct Summary {
    /// Seconds from letting go until the rim is at a third of opposite lock.
    to_counter_s: f64,
    /// Furthest past the car's lock, fraction of the lock.
    overshoot: f64,
    /// Separate times the rim went into the soft lock.
    wall_hits: usize,
    /// Reversals of the rim's direction while inside the soft lock.
    wall_reversals: usize,
}

fn summarise(run: &[(f64, f64, f64, f64)]) -> Summary {
    let to_counter_s = run
        .iter()
        .find(|r| r.1 < -1.0 / 3.0)
        .map_or(f64::INFINITY, |r| r.0);
    let overshoot = run.iter().map(|r| r.1.abs() - 1.0).fold(0.0, f64::max);
    let (mut wall_hits, mut wall_reversals, mut inside) = (0, 0, false);
    for w in run.windows(2) {
        let now = w[1].1.abs() > 1.0;
        if now && !inside {
            wall_hits += 1;
        }
        if now && inside && w[0].3.signum() != w[1].3.signum() {
            wall_reversals += 1;
        }
        inside = now;
    }
    Summary {
        to_counter_s,
        overshoot,
        wall_hits,
        wall_reversals,
    }
}

/// The rim's moment of inertia is not published for the CSL Elite; 0.02
/// kg.m2 is the estimate the hands-off parked-car test uses (D-089), and
/// 0.05 stands for a heavier rim. Both are unverified.
#[test]
fn let_go_in_a_slide_the_wheel_snaps_into_opposite_lock() {
    for j in [0.02, 0.05] {
        let m = summarise(&hands_off(j));
        eprintln!(
            "rim {j} kg.m2: a third of opposite lock after {:.2} s; {:.3} of lock past the wall, {} time(s) in, {} reversal(s) inside",
            m.to_counter_s, m.overshoot, m.wall_hits, m.wall_reversals
        );
        assert!(
            m.to_counter_s < 0.6,
            "rim {j}: opposite lock only after {:.2} s",
            m.to_counter_s
        );
        // At the wall: in, stopped by the base's full torque, back out.
        // No chatter against it.
        assert!(
            m.overshoot < 0.25,
            "rim {j}: {:.3} of lock past it",
            m.overshoot
        );
        assert!(
            m.wall_hits <= 2 && m.wall_reversals <= m.wall_hits,
            "rim {j}: {} hits, {} reversals at the soft lock",
            m.wall_hits,
            m.wall_reversals
        );
    }
}
