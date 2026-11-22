// SPDX-License-Identifier: Apache-2.0
//! The force-feedback grip-limit cue: the steering goes light before the
//! front tires reach their peak lateral force, so the driver feels the
//! limit coming. In the tire model the aligning moment is the lateral
//! force times a pneumatic trail that shrinks with slip (qts-tire,
//! magic.rs), so the moment peaks at a smaller slip angle than the force.
//! These tests check that on every tire file, through each car's steering
//! geometry (the mechanical trail adds a part that follows the force
//! itself), and on the moving car at the column.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::step::Simulation;
use qts_tire::magic::{steady_state, Operating};

const STEP_DEG: f64 = 0.05;

/// Slip angle sweep from 0 to 20 degrees: (angle deg, Fy N, Mz N·m).
fn sweep(
    p: &qts_tire::TireParams,
    surface: &qts_tire::SurfaceParams,
    fz: f64,
) -> Vec<(f64, f64, f64)> {
    (0..=(20.0 / STEP_DEG) as usize)
        .map(|i| {
            let a = i as f64 * STEP_DEG;
            let f = steady_state(p, surface, &Operating::simple(fz, 0.0, a.to_radians()));
            (a, f.fy, f.mz)
        })
        .collect()
}

fn argmax(v: impl Iterator<Item = (f64, f64)>) -> (f64, f64) {
    v.fold(
        (0.0, f64::MIN),
        |best, (x, y)| if y > best.1 { (x, y) } else { best },
    )
}

#[test]
fn aligning_moment_peaks_before_lateral_force_on_every_tire_file() {
    let root = data();
    let surface = root.load_surface("tarmac").unwrap().value;
    for id in root.list("tires").unwrap() {
        let p = root.load_tire(&id).unwrap().value;
        let fz0 = p.vertical.nominal_load_n;
        for fz in [0.5 * fz0, fz0, 1.5 * fz0] {
            let s = sweep(&p, &surface, fz);
            let (a_fy, _) = argmax(s.iter().map(|&(a, fy, _)| (a, fy.abs())));
            let (a_mz, mz_peak) = argmax(s.iter().map(|&(a, _, mz)| (a, mz.abs())));
            let mz_at_fy_peak = s.iter().find(|&&(a, _, _)| a == a_fy).unwrap().2.abs();
            assert!(
                a_mz < a_fy - 1.0,
                "{id} at {fz:.0} N: Mz peaks at {a_mz:.2} deg, Fy at {a_fy:.2} deg"
            );
            // The drop by the force peak must be large enough to feel.
            assert!(
                mz_at_fy_peak < 0.8 * mz_peak,
                "{id} at {fz:.0} N: Mz has only fallen to {:.0}% of its peak by the Fy peak",
                100.0 * mz_at_fy_peak / mz_peak
            );
        }
    }
}

#[test]
fn kingpin_moment_peaks_before_lateral_force_for_every_car_and_tire() {
    // The moment about the kingpin from one front tire: its own aligning
    // moment plus the lateral force on the mechanical trail (car.rs).
    let root = data();
    let surface = root.load_surface("tarmac").unwrap().value;
    for car in CARS {
        let c = root.load_car(car).unwrap().value;
        let b = &c.body;
        let g = qts_core::constants::GRAVITY_M_S2;
        let front_share = (b.wheelbase_m - b.cg_to_front_axle_m) / b.wheelbase_m;
        let fz = 0.5 * b.sprung_mass_kg * g * front_share + c.suspension.front.unsprung_mass_kg * g;
        let trail = c.steering.mechanical_trail_m;
        for id in root.list("tires").unwrap() {
            let p = root.load_tire(&id).unwrap().value;
            let s = sweep(&p, &surface, fz);
            let (a_fy, _) = argmax(s.iter().map(|&(a, fy, _)| (a, fy.abs())));
            let (a_k, _) = argmax(s.iter().map(|&(a, fy, mz)| (a, (-mz + trail * fy).abs())));
            assert!(
                a_k < a_fy - 0.5,
                "{car} on {id}: kingpin moment peaks at {a_k:.2} deg, Fy at {a_fy:.2} deg"
            );
        }
    }
}

#[test]
fn column_torque_peaks_before_front_lateral_force_on_the_moving_car() {
    // A slow steering ramp at a held 72 km/h: the torque at the column
    // (what the wheel sends to the driver) must peak, and start falling,
    // before the front axle's lateral force does.
    for car in CARS {
        let mut s = session(car, "test_track");
        let (x, y, h) = s.track().spawn();
        s.car_mut().place(x, y, h, 0.0);
        s.car_mut().set_forward_speed(20.0);
        let max = s.car().params().steering.max_wheel_angle_rad;
        let target = 300_f64.to_radians();
        let ticks = 12_000;
        let mut torque = Vec::with_capacity(ticks);
        let mut force = Vec::with_capacity(ticks);
        for t in 0..ticks {
            let angle = target * t as f64 / ticks as f64;
            // Hold the speed, so the ramp drives the fronts past their
            // peak rather than the car slowing onto a tighter line.
            let v = s.car().output().speed;
            let throttle = (0.3 + 0.6 * (20.0 - v)).clamp(0.0, 1.0);
            s.step(&InputFrame::from_analog(
                angle / max,
                throttle,
                0.0,
                0.0,
                0.0,
            ));
            let o = s.car().output();
            torque.push(o.steering_torque_nm.abs());
            force.push((o.wheels[0].fy + o.wheels[1].fy).abs());
        }
        // Smooth over 50 ms so a single noisy tick cannot decide the peak.
        let smooth = |v: &[f64]| -> Vec<f64> {
            v.windows(50)
                .map(|w| w.iter().sum::<f64>() / 50.0)
                .collect()
        };
        let (torque, force) = (smooth(&torque), smooth(&force));
        let peak = |v: &[f64]| argmax(v.iter().enumerate().map(|(i, &x)| (i as f64, x)));
        let (t_tq, tq_max) = peak(&torque);
        let (t_fy, fy_max) = peak(&force);
        let fy_at_tq = force[t_tq as usize];
        let tq_at_fy = torque[t_fy as usize];
        eprintln!(
            "{car}: torque peak {tq_max:.2} N.m at tick {t_tq} with front Fy at {:.0}% of its max; Fy max at tick {t_fy} with torque at {:.0}% of its peak",
            100.0 * fy_at_tq / fy_max,
            100.0 * tq_at_fy / tq_max
        );
        // The front force flattens onto a plateau near the limit, so the
        // exact tick of its maximum is noise; what matters is that the
        // torque peaks while the force is still building, and has fallen
        // clearly by the time the force is at its most.
        assert!(
            t_tq < t_fy,
            "{car}: torque peaks at tick {t_tq}, Fy at {t_fy}"
        );
        assert!(
            fy_at_tq < 0.95 * fy_max,
            "{car}: front Fy already at {:.0}% of its max when the torque peaks",
            100.0 * fy_at_tq / fy_max
        );
        assert!(
            tq_at_fy < 0.85 * tq_max,
            "{car}: torque only fell to {:.0}% of its peak by the Fy maximum",
            100.0 * tq_at_fy / tq_max
        );
    }
}
