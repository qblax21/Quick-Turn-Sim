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

/// One hands-off run of `ms` milliseconds; from `hold_from_s` on, if
/// given, the rim is held where it is instead (steering fixed). Returns
/// per-ms (time s, rim angle as a fraction of the car's lock, body slip
/// deg, rim rate rad/s, yaw rate rad/s, speed m/s).
fn run(
    rim_inertia: f64,
    base_friction_nm: f64,
    late_throttle: Option<f64>,
    ffb_damping: Option<f64>,
    ms: u32,
    hold_from_s: Option<f64>,
) -> Vec<(f64, f64, f64, f64, f64, f64)> {
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
    if let Some(d) = ffb_damping {
        cfg.damping_nm_s_per_rad = d;
    }
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
    for t in 0..ms {
        let read = (angle / step).round() * step;
        let slip = body_slip_deg(&s);
        let mut throttle = (0.6 + 0.03 * (25.0 - slip.abs())).clamp(0.1, 1.0);
        if let Some(late) = late_throttle.filter(|_| t >= 3000) {
            throttle = late;
        }
        s.step(&frame(
            matched_steer(read / (0.5 * rotation), rotation, lock),
            throttle,
        ));
        let sample = pipeline.step(&ffb_input(&s, Some(read)));
        late.push_back(f64::from(sample.device_command(&cfg)) * peak);
        let torque = late.pop_front().unwrap();
        if hold_from_s.is_some_and(|h| f64::from(t) * 0.001 >= h) {
            rate = 0.0;
        } else {
            // The base's own friction (belt and bearings), not published:
            // it opposes motion and holds a still rim against small torques.
            let moved = rate + torque / rim_inertia * 0.001;
            let slow = base_friction_nm / rim_inertia * 0.001;
            rate = if moved.abs() <= slow {
                0.0
            } else {
                moved - slow * moved.signum()
            };
        }
        angle += rate * 0.001;
        let o = s.car().output();
        out.push((
            f64::from(t) * 0.001,
            angle / lock,
            slip,
            rate,
            o.yaw_rate,
            o.drive.engine_rpm,
        ));
    }
    out
}

fn hands_off(rim_inertia: f64) -> Vec<(f64, f64, f64, f64, f64, f64)> {
    run(rim_inertia, 0.0, None, None, 3000, None)
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

fn summarise(run: &[(f64, f64, f64, f64, f64, f64)]) -> Summary {
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

/// Frequency (Hz) and damping ratio of the yaw-rate oscillation from
/// `from_s` on: zero crossings of the yaw rate about its mean, and the
/// logarithmic decrement of successive peaks. `None` without two cycles.
fn yaw_mode(run: &[(f64, f64, f64, f64, f64, f64)], from_s: f64) -> Option<(f64, f64, f64)> {
    let part: Vec<_> = run.iter().filter(|r| r.0 >= from_s).collect();
    let mean = part.iter().map(|r| r.4).sum::<f64>() / part.len() as f64;
    let mut crossings = Vec::new();
    for w in part.windows(2) {
        if (w[0].4 - mean) < 0.0 && (w[1].4 - mean) >= 0.0 {
            crossings.push(w[1].0);
        }
    }
    if crossings.len() < 3 {
        return None;
    }
    let period = (crossings[crossings.len() - 1] - crossings[0]) / (crossings.len() - 1) as f64;
    // Peak of each cycle.
    let peaks: Vec<f64> = crossings
        .windows(2)
        .map(|c| {
            part.iter()
                .filter(|r| r.0 >= c[0] && r.0 < c[1])
                .map(|r| (r.4 - mean).abs())
                .fold(0.0, f64::max)
        })
        .collect();
    let n = peaks.len() - 1;
    let delta = (peaks[0] / peaks[n].max(1e-9)).ln() / n as f64;
    let zeta = delta / (4.0 * std::f64::consts::PI.powi(2) + delta * delta).sqrt();
    let speed = part.iter().map(|r| r.5).sum::<f64>() / part.len() as f64;
    Some((1.0 / period, zeta, speed))
}

/// The weave after the slide (D-124). Let go of the wheel once the slide
/// is caught, throttle closed: with the preset's small force-feedback
/// damping (0.03 N.m.s/rad) the free rim and the car keep a yaw oscillation
/// going (0.8 Hz, about 0.8 rad/s; 1.9 Hz at 0.1); held steering shows none,
/// so the car itself is stable and the oscillation is the force-feedback
/// loop around a light free rim. At 0.2 N.m.s/rad it dies away. Real
/// hands-off (ISO 17288-1 steering release) behaviour depends on steering
/// friction and damping that are not published for this base, so physics
/// is not changed; the preset's damping waits for the F5 logs with the
/// gain.
#[test]
fn the_weave_after_a_slide_is_the_force_feedback_loop_not_the_car() {
    let held = run(0.02, 0.0, Some(0.0), None, 8000, Some(3.0));
    let amp = |r: &[(f64, f64, f64, f64, f64, f64)]| {
        r.iter()
            .filter(|x| x.0 >= 6.0)
            .map(|x| x.4.abs())
            .fold(0.0, f64::max)
    };
    eprintln!(
        "steering held: {:?}, yaw amplitude after 6 s {:.3} rad/s",
        yaw_mode(&held, 3.0),
        amp(&held)
    );
    let mut amps = Vec::new();
    for d in [0.03, 0.1, 0.2] {
        let free = run(0.02, 0.0, Some(0.0), Some(d), 8000, None);
        eprintln!(
            "hands off, FFB damping {d}: {:?} (Hz, damping ratio, mean rpm); yaw amplitude after 6 s {:.3} rad/s",
            yaw_mode(&free, 3.0),
            amp(&free)
        );
        amps.push(amp(&free));
    }
    // Held where it was at 3 s, the car turns steadily: no oscillation.
    assert!(
        yaw_mode(&held, 3.0).is_none(),
        "the car weaves with the steering held"
    );
    assert!(
        amps[0] > 0.3,
        "no weave at the preset's damping: {:.3}",
        amps[0]
    );
    assert!(
        amps[2] < 0.1,
        "still weaving at 0.2 N.m.s/rad: {:.3}",
        amps[2]
    );
}
