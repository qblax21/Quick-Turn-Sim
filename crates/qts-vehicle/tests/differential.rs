// SPDX-License-Identifier: Apache-2.0
//! The clutch-pack limited-slip differential, open and locked (D-084).

mod common;

use common::*;
use qts_tire::test_support::tarmac;
use qts_tire::SurfaceParams;
use qts_vehicle::car::{FL, FR};
use qts_vehicle::params::{DifferentialKind, DrivetrainParams};
use qts_vehicle::test_support::car_engine_manual;
use qts_vehicle::{Car, CarParams, DriverControls, Ground, GroundSample};

/// Grippy tarmac on the right of the world, ice on the left: the left
/// front wheel spins first.
struct SplitGround {
    grippy: SurfaceParams,
    ice: SurfaceParams,
}

impl Ground for SplitGround {
    fn sample(&self, _x: f64, y: f64) -> GroundSample<'_> {
        let surface = if y > 0.0 { &self.ice } else { &self.grippy };
        GroundSample {
            height: 0.0,
            normal: qts_core::math::Vec3::Z,
            surface,
            surface_index: u16::from(y > 0.0),
        }
    }
}

fn split() -> SplitGround {
    let mut ice = tarmac();
    ice.id = "ice".into();
    ice.friction_scale = 0.15;
    SplitGround {
        grippy: tarmac(),
        ice,
    }
}

fn with_diff(kind: DifferentialKind) -> CarParams {
    let mut p = car_engine_manual(true);
    if let DrivetrainParams::EngineManual(m) = &mut p.drivetrain {
        m.differential.kind = kind;
    }
    p
}

fn diff_of(car: &Car) -> qts_vehicle::params::DifferentialParams {
    match &car.params().drivetrain {
        DrivetrainParams::EngineManual(m) => m.differential.clone(),
        _ => unreachable!(),
    }
}

#[test]
fn ramp_angles_set_the_locking_per_newton_metre() {
    let d = diff_of(&build(with_diff(DifferentialKind::ClutchPack)));
    // mu n (r_c / r_p) / (2 tan theta) with 0.12, 8 faces, 1.0.
    assert!((d.lock_per_input(45.0) - 0.48).abs() < 1e-12);
    assert!((d.lock_per_input(60.0) - 0.48 / 60_f64.to_radians().tan()).abs() < 1e-12);
    assert!(d.lock_per_input(30.0) > d.lock_per_input(45.0));
    assert_eq!(d.lock_per_input(90.0), 0.0);
    // Power and coast ramps act on their own side of the input torque.
    assert!((d.locking_torque(100.0) - (d.preload_nm + 48.0)).abs() < 1e-9);
    let coast = d.preload_nm + 100.0 * d.lock_per_input(d.coast_ramp_deg);
    assert!((d.locking_torque(-100.0) - coast).abs() < 1e-9);
    // Lock percentages of older setups read back as ramp angles.
    for f in [0.1, 0.3, 0.48] {
        assert!((d.lock_per_input(d.ramp_for_lock(f)) - f).abs() < 1e-9);
    }
    let mut open = d.clone();
    open.kind = DifferentialKind::Open;
    assert_eq!(open.locking_torque(500.0), 0.0);
    let mut locked = d;
    locked.kind = DifferentialKind::Locked;
    assert!(locked.locking_torque(0.0).is_infinite());
}

/// Rolling in second on a split surface with the given throttle; returns
/// the mean (input, locking, transfer) torques over the last 0.3 s and the
/// last front wheel speeds.
fn on_split(kind: DifferentialKind, throttle: f64) -> ((f64, f64, f64), [f64; 2]) {
    let mut car = build(with_diff(kind));
    car.set_forward_speed(12.0);
    let g = split();
    let e = env();
    // Manual clutch and no traction control: the throttle alone sets the
    // torque into the differential.
    let assists = qts_vehicle::Assists {
        traction_control: false,
        ..MANUAL
    };
    let mut c = DriverControls {
        clutch: 1.0,
        gear_request: Some(2),
        ..DriverControls::default()
    };
    let mut sums = (0.0, 0.0, 0.0);
    let mut n = 0.0;
    for tick in 0..1500 {
        if tick == 300 {
            c.clutch = 0.0;
            c.throttle = throttle;
        }
        car.step(&c, &assists, &g, &e, DT);
        if tick >= 1200 {
            let d = car.output().drive;
            sums.0 += d.diff_input_nm;
            sums.1 += d.diff_lock_nm;
            sums.2 += d.diff_transfer_nm.abs();
            n += 1.0;
        }
    }
    let w = car.state.wheel_omega;
    ((sums.0 / n, sums.1 / n, sums.2 / n), [w[FL], w[FR]])
}

#[test]
fn locking_torque_rises_with_throttle_along_the_power_ramp() {
    let d = diff_of(&build(with_diff(DifferentialKind::ClutchPack)));
    let k = d.lock_per_input(d.power_ramp_deg);
    let mut last = 0.0;
    // Up to three quarters: at full throttle both fronts spin up, the
    // engine runs past its torque peak and less torque reaches the
    // differential (the lock still follows it, but no longer rises).
    for throttle in [0.25, 0.5, 0.75] {
        let ((input, lock, transfer), _) = on_split(DifferentialKind::ClutchPack, throttle);
        assert!(input > 0.0, "driving at throttle {throttle}");
        let expected = d.preload_nm + k * input;
        // Last tick's input sets this tick's lock: a small lag.
        assert!(
            (lock - expected).abs() < 0.03 * expected,
            "throttle {throttle}: lock {lock:.0} N.m, preload + {k:.2} x {input:.0} = {expected:.0}"
        );
        assert!(lock > last, "locking grows with throttle");
        assert!(transfer <= lock + 1e-6);
        last = lock;
    }
}

#[test]
fn on_overrun_the_coast_ramp_sets_the_locking() {
    let d = diff_of(&build(with_diff(DifferentialKind::ClutchPack)));
    let ((input, lock, _), _) = on_split(DifferentialKind::ClutchPack, 0.0);
    assert!(
        input < -5.0,
        "engine braking through the differential: {input:.1}"
    );
    let expected = d.preload_nm + d.lock_per_input(d.coast_ramp_deg) * input.abs();
    assert!(
        (lock - expected).abs() < 0.03 * expected,
        "lock {lock:.1}, expected {expected:.1}"
    );
    // Gentler than the drive side for the same torque.
    assert!(d.lock_per_input(d.coast_ramp_deg) < d.lock_per_input(d.power_ramp_deg));
}

#[test]
fn open_lets_the_ice_wheel_spin_and_locked_holds_them_together() {
    let ((_, lock, transfer), [l, r]) = on_split(DifferentialKind::Open, 1.0);
    assert_eq!(lock, 0.0);
    assert_eq!(transfer, 0.0);
    assert!(
        l > r * 1.2,
        "open: ice wheel {l:.1} rad/s, tarmac wheel {r:.1}"
    );
    let (_, [l2, r2]) = on_split(DifferentialKind::Locked, 1.0);
    assert!(
        (l2 - r2).abs() < 1e-3 * r2.abs().max(1.0),
        "locked: {l2} vs {r2}"
    );
    let (_, [l3, r3]) = on_split(DifferentialKind::ClutchPack, 1.0);
    assert!(l3 / r3 < l / r, "the clutch pack holds the ice wheel back");
}
