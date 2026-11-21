// SPDX-License-Identifier: Apache-2.0
//! Ten-minute headless soak: erratic inputs, every car, every track, with
//! the state checked for non-finite values and runaway every tick.

mod common;

use common::*;
use qts_core::input::buttons;
use qts_core::step::Simulation;
use qts_sim::drivers::{Driver, Erratic};
use qts_sim::Session;

const SOAK_SECONDS: u64 = 600;
/// Seconds without moving after which the soak driver resets the car.
const STUCK_SECONDS: u64 = 15;

/// Returns a description of the first thing found wrong with the state.
fn check(session: &Session) -> Result<(), String> {
    if !session.is_finite() {
        return Err("non-finite state".into());
    }
    let car = session.car();
    let s = &car.state;
    let out = car.output();
    let bad = |what: &str, value: f64| Err(format!("{what} out of range: {value}"));
    if s.pos.length() > 100_000.0 {
        return bad("position", s.pos.length());
    }
    if s.pos.z < -2.0 || s.pos.z > 30.0 {
        return bad("height", s.pos.z);
    }
    if s.vel.length() > 150.0 {
        return bad("speed", s.vel.length());
    }
    if s.ang_vel.length() > 60.0 {
        return bad("angular velocity", s.ang_vel.length());
    }
    for i in 0..4 {
        if s.susp[i].abs() > 0.6 {
            return bad("suspension travel", s.susp[i]);
        }
        if s.wheel_omega[i].abs() > 2500.0 {
            return bad("wheel speed", s.wheel_omega[i]);
        }
        let t = &s.tires[i].thermal;
        for temp in [t.surface_c, t.core_c, t.carcass_c, t.gas_c] {
            if !(-40.0..=600.0).contains(&temp) {
                return bad("tire temperature", temp);
            }
        }
        let w = &out.wheels[i];
        for v in [w.load, w.fx, w.fy, w.mz, w.slip_ratio, w.slip_angle] {
            if !v.is_finite() {
                return Err("non-finite tire output".into());
            }
        }
        if w.load > 80_000.0 {
            return bad("tire load", w.load);
        }
    }
    if !out.steering_torque_nm.is_finite() || out.steering_torque_nm.abs() > 2000.0 {
        return bad("steering torque", out.steering_torque_nm);
    }
    if !out.drive.engine_rpm.is_finite() || out.drive.engine_rpm > 30_000.0 {
        return bad("engine speed", out.drive.engine_rpm);
    }
    Ok(())
}

fn soak(car: &str, track: &str, seed: u64) {
    let mut session = session(car, track);
    let mut driver = Erratic::new(seed);
    let ticks = SOAK_SECONDS * u64::from(session.tick_rate_hz());
    let mut distance = 0.0;
    let mut max_speed: f64 = 0.0;
    let mut stuck_ticks = 0u64;
    let mut resets = 0u32;
    let stuck_limit = STUCK_SECONDS * u64::from(session.tick_rate_hz());
    for _ in 0..ticks {
        let mut frame = driver.frame(&session);
        // A driver pinned against a tree for a while resets the car, as a
        // person would; this also soaks the reset path.
        if stuck_ticks >= stuck_limit {
            frame.buttons |= buttons::RESET;
            stuck_ticks = 0;
            resets += 1;
        }
        session.step(&frame);
        if session.car().output().speed < 0.3 {
            stuck_ticks += 1;
        } else {
            stuck_ticks = 0;
        }
        if let Err(problem) = check(&session) {
            panic!(
                "{car} on {track} (seed {seed}): {problem} at tick {} ({:.3} s)",
                session.tick(),
                session.sim_time()
            );
        }
        let speed = session.car().output().speed;
        distance += speed * session.tick_dt();
        max_speed = max_speed.max(speed);
    }
    assert_eq!(session.tick(), ticks);
    // The soak is only meaningful if the car was really driven about. Among
    // solid trees a driver that never reverses still spends time held
    // against one before it resets, so less ground is asked.
    let needed = if session.track().trees().is_empty() {
        1500.0
    } else {
        500.0
    };
    assert!(
        distance > needed,
        "{car} on {track}: only covered {distance:.0} m"
    );
    assert!(
        max_speed > 15.0,
        "{car} on {track}: only reached {max_speed:.1} m/s"
    );
    println!(
        "soak {car} on {track}: {SOAK_SECONDS} s, {distance:.0} m, peak {max_speed:.1} m/s, {resets} resets, no instability"
    );
}

#[test]
fn ten_minutes_hatch_on_the_gravel_stage() {
    soak("hatch_tern_gt", "gravel_stage", 1);
}

#[test]
fn ten_minutes_limited_slip_hatch_on_the_test_track() {
    soak("hatch_tern_gt_lsd", "test_track", 2);
}

#[test]
fn ten_minutes_electric_sedan_on_the_autocross() {
    soak("sedan_halden_e4", "autocross_lot", 3);
}

#[test]
fn ten_minutes_electric_sedan_on_the_gravel_stage() {
    soak("sedan_halden_e4", "gravel_stage", 4);
}
