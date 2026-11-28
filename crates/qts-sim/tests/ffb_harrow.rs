// SPDX-License-Identifier: Apache-2.0
//! Force feedback at the grip limit on Harrow (D-135): the computer drives
//! a drift lap of Harrow in the Kestrel RC, and every tick's steering
//! torque is set against the front tires' slip angle. Nothing here is a
//! canned effect: the torque is the kingpin moment from the tire model
//! (qts-vehicle, car.rs), through the steering and the force-feedback
//! pipeline.
//!
//! What it shows:
//! - **Light before the limit:** the centring torque peaks at a smaller
//!   front slip angle than the front lateral force (D-104's cue, here on a
//!   real lap).
//! - **Front scrub:** past the force's peak the torque keeps falling, so a
//!   pushing front end feels light.
//! - **Rear breakaway through the rack, and self-steer:** the car's yaw
//!   turns the fronts' slip angle round, so in a slide the torque pulls the
//!   wheel into opposite lock, even with the rim already countersteered
//!   (D-116 checks the hands-off version).
//!
//! `cargo test -p qts-sim --test ffb_harrow -- --nocapture` prints the
//! torque against slip angle table.

mod common;

use common::*;
use qts_core::step::Simulation;
use qts_sim::ai::{AiConfig, AiDriver};
use qts_sim::drivers::Driver;
use qts_sim::host::ffb_input;
use qts_sim::session::Session;

const CAR: &str = "coupe_kestrel_rc";
const TRACK: &str = "harrow_drift_park";
/// Slip angle bins, degrees wide.
const BIN_DEG: f64 = 1.0;
const BINS: usize = 16;

struct Tick {
    /// Front slip angle, mean of the two fronts, deg.
    front_deg: f64,
    /// Rear slip angle, mean of the two rears, deg.
    rear_deg: f64,
    /// Front lateral force, N.
    front_fy: f64,
    /// Raw column torque, N·m, positive turning the wheel left.
    torque: f64,
    /// The torque at the wheel base after the pipeline, N·m.
    device: f64,
    /// Steering-wheel angle, rad, positive left.
    rim: f64,
    /// Sideways body velocity, m/s, positive left.
    vy: f64,
    speed: f64,
}

fn lap() -> Vec<Tick> {
    let root = data();
    let mut s = Session::from_data(&root, CAR, TRACK, AiDriver::session_config()).unwrap();
    let mut ai = AiDriver::new(s.track(), AiConfig::drift(0.85)).unwrap();
    let cfg = root.load_ffb("fanatec_csl_elite").unwrap().value;
    let peak = cfg.device_max_torque_nm;
    let mut pipeline = qts_ffb::FfbPipeline::new(cfg.clone(), s.tick_rate_hz()).unwrap();
    let rate = f64::from(s.tick_rate_hz());
    let mut out = Vec::new();
    // About one lap: Harrow's drift lap takes the computer about two
    // minutes.
    while (s.tick() as f64) < 120.0 * rate {
        let f = ai.frame(&s);
        s.step(&f);
        let sample = pipeline.step(&ffb_input(&s, None));
        let o = s.car().output();
        let w = &o.wheels;
        out.push(Tick {
            front_deg: (0.5 * (w[0].slip_angle + w[1].slip_angle)).to_degrees(),
            rear_deg: (0.5 * (w[2].slip_angle + w[3].slip_angle)).to_degrees(),
            front_fy: w[0].fy + w[1].fy,
            torque: o.steering_torque_nm,
            device: f64::from(sample.device_command(&cfg)) * peak,
            rim: o.steering_wheel_angle,
            vy: o.vel_body[1],
            speed: o.speed,
        });
    }
    out
}

/// A push on Harrow: from the start at 15 m/s the wheel is wound on over
/// 1.2 s with the speed held, past what the fronts can take, so they scrub.
/// Ticks off the start's surface are left out. Returns the same ticks.
fn push() -> Vec<Tick> {
    let root = data();
    let mut s = Session::from_data(&root, CAR, TRACK, AiDriver::session_config()).unwrap();
    let (x, y, h) = s.track().spawn();
    s.car_mut().place(x, y, h, 0.0);
    s.car_mut().set_forward_speed(15.0);
    let ticks = 1600;
    let mut out = Vec::new();
    let mut surface = None;
    for t in 0..ticks {
        let v = s.car().output().speed;
        let throttle = (0.25 + 0.5 * (15.0 - v)).clamp(0.0, 0.6);
        s.step(&qts_core::input::InputFrame::from_analog(
            (0.9 * t as f64 / 1200.0).min(0.9),
            throttle,
            0.0,
            0.0,
            0.0,
        ));
        let o = s.car().output();
        let w = &o.wheels;
        let here = (w[0].surface_index, w[1].surface_index);
        if *surface.get_or_insert(here) != here {
            continue;
        }
        out.push(Tick {
            front_deg: (0.5 * (w[0].slip_angle + w[1].slip_angle)).to_degrees(),
            rear_deg: (0.5 * (w[2].slip_angle + w[3].slip_angle)).to_degrees(),
            front_fy: w[0].fy + w[1].fy,
            torque: o.steering_torque_nm,
            device: 0.0,
            rim: o.steering_wheel_angle,
            vy: o.vel_body[1],
            speed: o.speed,
        });
    }
    out
}

/// The centring part of the torque: the aligning torque turns the wheel
/// against the front slip, so this is positive when it centres.
fn centring(t: &Tick) -> f64 {
    -t.torque * t.front_deg.signum()
}

#[test]
fn harrow_push_scrubs_the_front() {
    // Front scrub: wound on past the fronts' peak, the torque falls away
    // while the force stays near its top, so the steering goes light.
    let ticks = push();
    let smooth = |f: &dyn Fn(&Tick) -> f64| -> Vec<f64> {
        ticks
            .windows(25)
            .map(|w| w.iter().map(f).sum::<f64>() / 25.0)
            .collect()
    };
    let (tq, fy) = (smooth(&centring), smooth(&|t: &Tick| t.front_fy.abs()));
    let slip: Vec<f64> = smooth(&|t: &Tick| t.front_deg.abs());
    let at = |v: &[f64]| (0..v.len()).max_by(|&a, &b| v[a].total_cmp(&v[b])).unwrap();
    let (i_tq, i_fy) = (at(&tq), at(&fy));
    let scrub = slip
        .iter()
        .position(|&a| a >= 2.0 * slip[i_fy])
        .unwrap_or(slip.len() - 1);
    eprintln!("push, Kestrel RC on Harrow (front slip, column torque centring, front Fy):");
    for k in (0..slip.len()).step_by(100) {
        eprintln!("  {:5.1} deg  {:6.2} N.m  {:6.0} N", slip[k], tq[k], fy[k]);
    }
    eprintln!(
        "torque peak {:.2} N.m at {:.1} deg; Fy peak {:.0} N at {:.1} deg with the torque at {:.0}%; at {:.1} deg the torque is {:.0}% and Fy {:.0}%",
        tq[i_tq], slip[i_tq], fy[i_fy], slip[i_fy],
        100.0 * tq[i_fy] / tq[i_tq], slip[scrub], 100.0 * tq[scrub] / tq[i_tq], 100.0 * fy[scrub] / fy[i_fy]
    );
    assert!(
        slip[i_tq] < slip[i_fy],
        "the torque must peak before the force"
    );
    // Only a little lighter at the force's peak: the angle kit's long
    // mechanical trail (38 mm) carries much of the torque on a part that
    // follows the force (D-104, D-111). The drop comes while scrubbing.
    assert!(
        tq[i_fy] < 0.97 * tq[i_tq],
        "already falling at the force's peak"
    );
    assert!(tq[scrub] < 0.85 * tq[i_tq], "lighter still while scrubbing");
    assert!(
        fy[scrub] > 0.9 * fy[i_fy],
        "the force holds near its top meanwhile"
    );
}

#[test]
fn harrow_torque_against_front_slip_angle() {
    let ticks = lap();
    // The grip curve: ticks with the rear still gripping, so the front slip
    // angle is the driver's steering, not the car's yaw.
    let mut n = [0usize; BINS];
    let mut tq = [0.0_f64; BINS];
    let mut dev = [0.0_f64; BINS];
    let mut fy = [0.0_f64; BINS];
    for t in &ticks {
        if t.speed < 8.0 || t.rear_deg.abs() > 4.0 {
            continue;
        }
        let b = (t.front_deg.abs() / BIN_DEG) as usize;
        if b >= BINS {
            continue;
        }
        let sign = t.front_deg.signum();
        n[b] += 1;
        tq[b] -= t.torque * sign;
        dev[b] -= t.device * sign;
        fy[b] += t.front_fy * sign;
    }
    eprintln!("Harrow lap, rear gripping: front slip, ticks, centring torque at the column and at the base, front Fy");
    let mean = |v: &[f64; BINS], b: usize| v[b] / n[b].max(1) as f64;
    for b in 0..BINS {
        if n[b] == 0 {
            continue;
        }
        eprintln!(
            "{:4.0}-{:<4.0} {:6}  {:8.2} N.m  {:6.2} N.m  {:6.0} N",
            b as f64 * BIN_DEG,
            (b + 1) as f64 * BIN_DEG,
            n[b],
            mean(&tq, b),
            mean(&dev, b),
            mean(&fy, b).abs()
        );
    }
    // Bins with enough ticks to judge by (200 ms of driving).
    let full: Vec<usize> = (0..BINS).filter(|&b| n[b] >= 200).collect();
    let peak = *full
        .iter()
        .max_by(|&&a, &&b| mean(&tq, a).total_cmp(&mean(&tq, b)))
        .unwrap();
    let last = *full.last().unwrap();
    assert!(peak < last, "the torque peaks inside the range driven");
    assert!(
        mean(&fy, last).abs() > mean(&fy, peak).abs(),
        "the force is still rising past the torque's peak"
    );
    assert!(
        mean(&tq, last) < 0.9 * mean(&tq, peak),
        "and the torque has fallen by then"
    );

    // Rear breakaway and the slide. The car yawing turns the fronts'
    // slip angle round, so in a slide the aligning torque points the
    // wheel into opposite lock. Counted where the rim is already
    // countersteered, so ordinary centring cannot pass for it.
    let counter: Vec<&Tick> = ticks
        .iter()
        .filter(|t| t.rear_deg.abs() > 10.0 && t.speed > 8.0 && t.rim.signum() == t.vy.signum())
        .collect();
    let further = counter
        .iter()
        .filter(|t| t.torque.signum() == t.vy.signum())
        .count();
    let share = further as f64 / counter.len().max(1) as f64;
    eprintln!(
        "sliding with the rim countersteered: {} ticks, torque pulling further into opposite lock in {:.0}%",
        counter.len(),
        100.0 * share
    );
    assert!(counter.len() > 5000, "the lap slides");
    assert!(share > 0.8);
}
