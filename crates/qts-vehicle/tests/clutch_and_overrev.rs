// SPDX-License-Identifier: Apache-2.0
//! Clutch pedal curve, rev limiter and over-rev damage.

mod common;

use common::*;
use qts_vehicle::params::{DrivetrainParams, BITE_TOP_ENGAGEMENT};
use qts_vehicle::test_support::car_engine_manual;
use qts_vehicle::{Assists, DriverControls};

const DAMAGE: Assists = Assists {
    engine_damage: true,
    ..MANUAL
};

fn manual_params() -> qts_vehicle::params::EngineManualParams {
    match car_engine_manual(false).drivetrain {
        DrivetrainParams::EngineManual(m) => m,
        _ => unreachable!(),
    }
}

#[test]
fn the_pedal_has_free_travel_a_bite_zone_and_a_free_floor() {
    let cl = manual_params().clutch;
    let release = cl.release_travel();
    let bite_top = cl.bite_point - 0.5 * cl.bite_width;
    assert_eq!(cl.engagement(0.0), 1.0);
    assert_eq!(
        cl.engagement(cl.free_travel),
        1.0,
        "free travel does nothing"
    );
    assert!((cl.engagement(bite_top) - BITE_TOP_ENGAGEMENT).abs() < 1e-12);
    assert_eq!(cl.engagement(release), 0.0);
    assert_eq!(cl.engagement(1.0), 0.0, "free well before the floor");
    assert!(release < 0.95);
    // Monotonic and continuous: no step bigger than the slope allows.
    let mut prev = cl.engagement(0.0);
    for i in 1..=10_000 {
        let e = cl.engagement(f64::from(i) / 10_000.0);
        assert!(e <= prev + 1e-12, "rises at {i}");
        assert!(prev - e < 1e-3, "jumps at {i}: {prev} to {e}");
        prev = e;
    }
    // The first touch is gentle: a tenth of the zone above the release
    // point gives a small share of the capacity.
    let first_touch = cl.engagement(release - 0.1 * cl.bite_width);
    assert!(first_touch < 0.01, "first touch {first_touch}");
}

/// First gear selected with the clutch floored, then the pedal held at
/// `travel` with some throttle for `seconds`. Returns the car.
fn hold_pedal(travel: f64, seconds: f64) -> qts_vehicle::Car {
    let mut car = build(car_engine_manual(false));
    run(&mut car, &DriverControls::default(), &MANUAL, 0.5);
    let mut c = DriverControls {
        clutch: 1.0,
        gear_request: Some(1),
        throttle: 0.25,
        ..DriverControls::default()
    };
    run(&mut car, &c, &MANUAL, 0.5);
    assert_eq!(car.output().drive.gear, 1);
    c.clutch = travel;
    run(&mut car, &c, &MANUAL, seconds);
    car
}

#[test]
fn below_the_bite_zone_nothing_drives_and_in_it_the_clutch_slips() {
    let cl = manual_params().clutch;
    // Past the release point: the car stays put however long it waits.
    let car = hold_pedal(cl.release_travel() + 0.05, 2.0);
    assert!(car.output().speed < 0.01, "moved with the clutch free");
    assert_eq!(car.output().drive.clutch_engagement, 0.0);
    // Low in the bite zone: drive, with the clutch slipping.
    let car = hold_pedal(cl.release_travel() - 0.3 * cl.bite_width, 0.6);
    let d = car.output().drive;
    assert!(d.clutch_engagement > 0.0 && d.clutch_engagement < BITE_TOP_ENGAGEMENT);
    assert!(
        d.clutch_torque_nm > 1.0,
        "no drive: {} N.m",
        d.clutch_torque_nm
    );
    assert!(
        d.clutch_slip_rad_s > 5.0,
        "should slip, {}",
        d.clutch_slip_rad_s
    );
    assert!(!d.stalled);
    // In the free travel the clutch is clamped: once the car is rolling
    // the plates lock.
    let car = hold_pedal(0.5 * cl.free_travel, 3.0);
    let d = car.output().drive;
    assert_eq!(d.clutch_engagement, 1.0);
    assert!(
        d.clutch_slip_rad_s.abs() < 0.5,
        "slip {}",
        d.clutch_slip_rad_s
    );
}

#[test]
fn moving_the_bite_point_moves_where_the_car_starts_to_pull() {
    // The same pedal position is free with a high bite point and drives
    // with a low one.
    let travel = 0.5;
    let mut early = car_engine_manual(false);
    let mut late = car_engine_manual(false);
    if let (DrivetrainParams::EngineManual(e), DrivetrainParams::EngineManual(l)) =
        (&mut early.drivetrain, &mut late.drivetrain)
    {
        e.clutch.bite_point = 0.60;
        l.clutch.bite_point = 0.30;
    }
    let mut torque = Vec::new();
    for params in [early, late] {
        let mut car = build(params);
        run(&mut car, &DriverControls::default(), &MANUAL, 0.5);
        let mut c = DriverControls {
            clutch: 1.0,
            gear_request: Some(1),
            throttle: 0.25,
            ..DriverControls::default()
        };
        run(&mut car, &c, &MANUAL, 0.5);
        c.clutch = travel;
        run(&mut car, &c, &MANUAL, 0.3);
        torque.push(car.output().drive.clutch_torque_nm);
    }
    assert!(torque[0] > 5.0, "bite point 60%: {} N.m", torque[0]);
    assert!(torque[1].abs() < 1e-9, "bite point 30%: {} N.m", torque[1]);
}

#[test]
fn the_limiter_holds_a_free_revving_engine_and_harms_nothing() {
    let mut car = build(car_engine_manual(false));
    let c = DriverControls {
        throttle: 1.0,
        ..DriverControls::default()
    };
    run(&mut car, &c, &DAMAGE, 4.0);
    let d = car.output().drive;
    let redline = manual_params().engine.redline_rpm;
    assert!(d.peak_rpm > redline - 200.0, "never reached the limiter");
    assert!(
        d.peak_rpm < redline + 300.0,
        "limiter overshoot {}",
        d.peak_rpm
    );
    assert!(!d.overrev);
    assert_eq!(d.engine_damage, 0.0);
}

/// Up to 26 m/s (about 58 mph), then clutch down, first gear, clutch
/// dropped: the classic money shift. First gear at that speed is far past
/// the failure speed, whatever gear the run-up ended in. Returns the car
/// just after.
fn missed_downshift(assists: &Assists) -> qts_vehicle::Car {
    let mut car = build(car_engine_manual(false));
    let c = controls(0.0, 1.0, 0.0);
    let (g, e) = (ground(), env());
    for _ in 0..30_000 {
        car.step(&c, &AUTO, &g, &e, DT);
        if car.output().speed > 26.0 {
            break;
        }
    }
    assert!(car.output().speed > 26.0);
    let mut c = DriverControls {
        clutch: 1.0,
        gear_request: Some(1),
        ..DriverControls::default()
    };
    run(&mut car, &c, assists, 0.4);
    assert_eq!(car.output().drive.gear, 1);
    c.clutch = 0.0;
    run(&mut car, &c, assists, 0.6);
    car
}

#[test]
fn a_missed_downshift_over_revs_past_the_limiter_and_is_logged() {
    let p = manual_params().engine;
    let car = missed_downshift(&MANUAL);
    let d = car.output().drive;
    assert!(
        d.peak_rpm > p.overrev_damage_rpm,
        "peak only {} rpm",
        d.peak_rpm
    );
    // Damage off: reported, but the engine is unharmed and still runs.
    assert_eq!(d.engine_damage, 0.0);
    assert!(!d.stalled);
}

#[test]
fn with_damage_on_a_big_over_rev_destroys_the_engine() {
    let p = manual_params().engine;
    let mut car = missed_downshift(&DAMAGE);
    let d = car.output().drive;
    assert!(
        d.peak_rpm > p.overrev_failure_rpm,
        "peak {} rpm",
        d.peak_rpm
    );
    assert_eq!(d.engine_damage, 1.0);
    // A failed engine is dead: no starter, no bump start.
    let c = DriverControls {
        clutch: 1.0,
        gear_request: Some(0),
        starter: true,
        throttle: 1.0,
        ..DriverControls::default()
    };
    run(&mut car, &c, &DAMAGE, 12.0);
    assert!(car.output().drive.stalled);
    assert!(car.output().drive.engine_rpm < 400.0);
    // A reset repairs it.
    car.place(0.0, 0.0, 0.0, 0.0);
    run(&mut car, &DriverControls::default(), &DAMAGE, 1.0);
    let d = car.output().drive;
    assert_eq!(d.engine_damage, 0.0);
    assert!(!d.stalled && d.engine_rpm > 700.0, "{d:?}");
}
