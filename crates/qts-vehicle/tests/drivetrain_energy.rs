// SPDX-License-Identifier: Apache-2.0
//! Drivetrain modules and energy sanity checks.

mod common;

use common::*;
use qts_tire::test_support::tarmac;
use qts_tire::SurfaceParams;
use qts_vehicle::car::{FL, FR, RL, RR};
use qts_vehicle::params::DrivetrainParams;
use qts_vehicle::test_support::{car_dual_motor, car_engine_manual};
use qts_vehicle::{DriverControls, Ground, GroundSample};

/// Tarmac on the right half of the world, a slippery surface on the left.
struct SplitGround {
    grippy: SurfaceParams,
    slippery: SurfaceParams,
}

impl SplitGround {
    fn new() -> Self {
        let mut slippery = tarmac();
        slippery.id = "ice".into();
        slippery.friction_scale = 0.15;
        Self {
            grippy: tarmac(),
            slippery,
        }
    }
}

impl Ground for SplitGround {
    fn sample(&self, _x: f64, y: f64) -> GroundSample<'_> {
        if y > 0.0 {
            GroundSample {
                height: 0.0,
                normal: qts_core::math::Vec3::Z,
                surface: &self.slippery,
                surface_index: 1,
            }
        } else {
            GroundSample {
                height: 0.0,
                normal: qts_core::math::Vec3::Z,
                surface: &self.grippy,
                surface_index: 0,
            }
        }
    }
}

#[test]
fn engine_car_pulls_away_and_shifts_up_with_assists() {
    let mut car = build(car_engine_manual(false));
    let g = ground();
    let e = env();
    let c = controls(0.0, 1.0, 0.0);
    let redline = match &car.params().drivetrain {
        DrivetrainParams::EngineManual(d) => d.engine.redline_rpm,
        _ => unreachable!(),
    };
    let mut max_rpm: f64 = 0.0;
    for _ in 0..12_000 {
        car.step(&c, &AUTO, &g, &e, DT);
        max_rpm = max_rpm.max(car.output().drive.engine_rpm);
    }
    let out = car.output();
    assert!(out.speed > 30.0, "speed after 12 s: {}", out.speed);
    assert!(out.drive.gear >= 3, "gear {}", out.drive.gear);
    assert!(max_rpm < redline + 400.0, "over-rev to {max_rpm}");
}

#[test]
fn manual_clutch_and_gear_lever_work_in_both_directions() {
    let mut car = build(car_engine_manual(false));
    // Idle in neutral: nothing moves.
    run(&mut car, &DriverControls::default(), &MANUAL, 1.0);
    assert!(car.output().speed < 0.01);
    let idle = car.output().drive.engine_rpm;
    assert!((idle - 850.0).abs() < 80.0, "idle at {idle} rpm");

    // Clutch in, select first, rev, let the clutch out progressively.
    let mut c = DriverControls {
        clutch: 1.0,
        gear_request: Some(1),
        throttle: 0.4,
        ..DriverControls::default()
    };
    run(&mut car, &c, &MANUAL, 0.6);
    assert!(
        car.output().speed < 0.05,
        "must not move with the clutch pressed"
    );
    assert_eq!(car.output().drive.gear, 1);
    let g = ground();
    let e = env();
    for i in 0..1500 {
        c.clutch = 1.0 - i as f64 / 1500.0;
        car.step(&c, &MANUAL, &g, &e, DT);
    }
    c.clutch = 0.0;
    run(&mut car, &c, &MANUAL, 1.5);
    assert!(
        car.output().vel_body[0] > 3.0,
        "forward speed {}",
        car.output().vel_body[0]
    );

    // Reverse from rest.
    let mut car = build(car_engine_manual(false));
    let mut c = DriverControls {
        clutch: 1.0,
        gear_request: Some(-1),
        throttle: 0.4,
        ..DriverControls::default()
    };
    run(&mut car, &c, &MANUAL, 0.6);
    for i in 0..1500 {
        c.clutch = 1.0 - i as f64 / 1500.0;
        car.step(&c, &MANUAL, &g, &e, DT);
    }
    c.clutch = 0.0;
    run(&mut car, &c, &MANUAL, 1.0);
    assert!(
        car.output().vel_body[0] < -1.0,
        "reverse speed {}",
        car.output().vel_body[0]
    );
}

#[test]
fn stopping_in_gear_stalls_and_only_the_starter_restarts_the_engine() {
    let mut car = build(car_engine_manual(false));
    // Clutch in, first gear, then the clutch let out against the brakes.
    let mut c = DriverControls {
        clutch: 1.0,
        gear_request: Some(1),
        brake: 1.0,
        ..DriverControls::default()
    };
    run(&mut car, &c, &MANUAL, 0.5);
    assert_eq!(car.output().drive.gear, 1);
    c.clutch = 0.0;
    run(&mut car, &c, &MANUAL, 2.0);
    let d = car.output().drive;
    assert!(d.stalled, "the engine should stall");
    assert!(d.engine_rpm < 50.0, "{} rpm", d.engine_rpm);
    // Clutch down is not enough: a stalled engine stays stalled.
    c.clutch = 1.0;
    run(&mut car, &c, &MANUAL, 1.0);
    assert!(car.output().drive.stalled);
    // The starter will not turn it with the clutch up in gear.
    c.clutch = 0.0;
    c.starter = true;
    run(&mut car, &c, &MANUAL, 1.0);
    assert!(car.output().drive.stalled, "starter interlock");
    // Clutch down and the starter: it fires and settles at idle.
    c.clutch = 1.0;
    run(&mut car, &c, &MANUAL, 0.6);
    c.starter = false;
    run(&mut car, &c, &MANUAL, 1.5);
    let d = car.output().drive;
    assert!(
        !d.stalled && d.engine_rpm > 700.0,
        "restarted at {} rpm",
        d.engine_rpm
    );
}

#[test]
fn a_gear_forced_in_without_the_clutch_grinds_and_stays_in_neutral() {
    let mut car = build(car_engine_manual(false));
    run(&mut car, &DriverControls::default(), &MANUAL, 0.5);
    // Engine at idle, car at rest: first gear without the clutch.
    let mut c = DriverControls {
        gear_request: Some(1),
        ..DriverControls::default()
    };
    run(&mut car, &c, &MANUAL, 0.5);
    let d = car.output().drive;
    assert!(d.grinding, "should grind");
    assert_eq!(d.gear, 0, "stays in neutral");
    assert!(!d.stalled);
    assert!(car.output().speed < 0.01);
    // The clutch down lets it in.
    c.clutch = 1.0;
    run(&mut car, &c, &MANUAL, 0.5);
    let d = car.output().drive;
    assert!(!d.grinding);
    assert_eq!(d.gear, 1);
}

#[test]
fn a_rev_matched_gear_goes_in_without_the_clutch() {
    let mut car = build(car_engine_manual(false));
    // Under way in second with the automatic clutch's help.
    let c = DriverControls {
        throttle: 0.6,
        gear_request: Some(2),
        ..DriverControls::default()
    };
    run(&mut car, &c, &AUTO, 4.0);
    assert_eq!(car.output().drive.gear, 2);
    // Out to neutral without the clutch, and straight back into second:
    // the engine is still turning at second gear's speed.
    let mut c = DriverControls {
        gear_request: Some(0),
        ..DriverControls::default()
    };
    run(&mut car, &c, &MANUAL, 0.03);
    assert_eq!(car.output().drive.gear, 0);
    c.gear_request = Some(2);
    run(&mut car, &c, &MANUAL, 0.3);
    let d = car.output().drive;
    assert!(
        !d.grinding && d.gear == 2,
        "matched shift refused: gear {} grinding {}",
        d.gear,
        d.grinding
    );
    // Fourth is far too tall for this speed: refused.
    c.gear_request = Some(0);
    run(&mut car, &c, &MANUAL, 0.3);
    c.gear_request = Some(4);
    run(&mut car, &c, &MANUAL, 0.05);
    assert!(car.output().drive.grinding);
}

#[test]
fn a_stalled_engine_bump_starts_when_the_wheels_turn_it() {
    let mut car = build(car_engine_manual(false));
    // Under way in second.
    let c = DriverControls {
        throttle: 0.7,
        gear_request: Some(2),
        ..DriverControls::default()
    };
    run(&mut car, &c, &AUTO, 4.0);
    // Clutch down, sixth, and the clutch dropped at low speed: the engine
    // is dragged below its stall speed while the car rolls on.
    let mut c = DriverControls {
        clutch: 1.0,
        gear_request: Some(6),
        brake: 0.6,
        ..DriverControls::default()
    };
    let (g, e) = (ground(), env());
    for _ in 0..5000 {
        car.step(&c, &MANUAL, &g, &e, DT);
        if car.output().speed < 4.0 {
            break;
        }
    }
    c.brake = 0.0;
    c.clutch = 0.0;
    run(&mut car, &c, &MANUAL, 0.6);
    let rolling = car.output().speed;
    assert!(
        car.output().drive.stalled,
        "stall in sixth at {rolling:.1} m/s, {} rpm",
        car.output().drive.engine_rpm
    );
    assert!(rolling > 1.5, "still rolling at {rolling:.1} m/s");
    // Clutch down, second, clutch out: the wheels turn it and it fires.
    c.clutch = 1.0;
    c.gear_request = Some(2);
    run(&mut car, &c, &MANUAL, 0.3);
    c.clutch = 0.0;
    run(&mut car, &c, &MANUAL, 0.5);
    let d = car.output().drive;
    assert!(!d.stalled, "bump start failed at {} rpm", d.engine_rpm);
}

#[test]
fn limited_slip_gets_more_drive_on_a_split_surface() {
    let launch = |lsd: bool| {
        let mut car = build(car_engine_manual(lsd));
        let g = SplitGround::new();
        let e = env();
        let c = DriverControls {
            throttle: 1.0,
            gear_request: Some(1),
            ..DriverControls::default()
        };
        let mut spin: f64 = 0.0;
        for _ in 0..3000 {
            car.step(&c, &AUTO, &g, &e, DT);
            let w = car.output().wheels;
            spin = spin.max((w[FL].omega - w[FR].omega).abs());
        }
        (car.output().vel_body[0], spin)
    };
    let (v_open, spin_open) = launch(false);
    let (v_lsd, spin_lsd) = launch(true);
    assert!(
        v_lsd > v_open * 1.15,
        "open {v_open:.2} m/s, limited slip {v_lsd:.2} m/s"
    );
    assert!(spin_lsd < spin_open, "wheel speed difference should shrink");
}

#[test]
fn electric_car_drives_all_four_wheels_and_reverses() {
    let mut car = build(car_dual_motor());
    run(&mut car, &controls(0.0, 1.0, 0.0), &AUTO, 3.0);
    let out = *car.output();
    assert!(out.speed > 20.0, "speed after 3 s: {}", out.speed);
    for w in out.wheels {
        assert!(w.drive_torque > 50.0, "every wheel should be driven");
    }
    assert!(out.drive.motor_torque_nm[0] > 0.0 && out.drive.motor_torque_nm[1] > 0.0);
    assert!(out.drive.battery_power_w > 0.0);

    let mut car = build(car_dual_motor());
    let c = DriverControls {
        throttle: 0.3,
        gear_request: Some(-1),
        ..DriverControls::default()
    };
    run(&mut car, &c, &AUTO, 2.0);
    assert!(car.output().vel_body[0] < -2.0);
}

#[test]
fn regeneration_takes_over_part_of_the_braking_and_charges_the_battery() {
    let brake_from_speed = |brake_share: f64| {
        let mut p = car_dual_motor();
        if let DrivetrainParams::DualMotorElectric(d) = &mut p.drivetrain {
            d.regen.brake_share = brake_share;
            d.regen.coast_fraction = 0.0;
        }
        let mut car = build(p);
        car.set_forward_speed(25.0);
        let soc_before = car.output().drive.battery_soc;
        run(&mut car, &controls(0.0, 0.0, 0.3), &AUTO, 1.0);
        let out = *car.output();
        let friction: f64 = out.wheels.iter().map(|w| w.brake_torque.abs()).sum();
        (
            -out.accel_body[0],
            friction,
            out.drive.regen_power_w,
            out.drive.battery_soc - soc_before,
        )
    };
    let (decel_blend, friction_blend, regen_power, soc_gain) = brake_from_speed(0.7);
    let (decel_plain, friction_plain, plain_regen, _) = brake_from_speed(0.0);
    assert!(plain_regen.abs() < 1.0);
    assert!(regen_power > 5000.0, "regen power {regen_power}");
    assert!(soc_gain > 0.0, "battery should gain charge");
    assert!(
        friction_blend < 0.7 * friction_plain,
        "{friction_blend} vs {friction_plain}"
    );
    assert!(
        (decel_blend - decel_plain).abs() < 0.08 * decel_plain,
        "pedal feel should not change: {decel_blend} vs {decel_plain}"
    );
}

#[test]
fn regeneration_yields_to_the_friction_brakes_when_anti_lock_intervenes() {
    let mut car = build(car_dual_motor());
    car.set_forward_speed(30.0);
    run(&mut car, &controls(0.0, 0.0, 1.0), &AUTO, 1.0);
    assert!(car.output().drive.regen_power_w < 1000.0);
    assert!(-car.output().accel_body[0] > 7.0);
}

#[test]
fn a_passive_car_never_gains_mechanical_energy() {
    // Neutral gear, no pedals: nothing can add energy. Start with the car
    // dropped from a height, sliding sideways and yawing so every
    // dissipative element is exercised.
    let mut car = build(car_engine_manual(false));
    car.set_forward_speed(25.0);
    car.state.pos.z += 0.05;
    car.state.vel.y = 3.0;
    car.state.ang_vel.z = 0.8;
    let g = ground();
    let e = env();
    let gravity = e.gravity;
    let c = controls(0.1, 0.0, 0.0);
    let energy = |car: &qts_vehicle::Car| {
        car.mechanical_energy(&g, gravity) - car.drivetrain().kinetic_energy()
    };
    let start = energy(&car);
    let mut previous = start;
    let mut worst_gain: f64 = 0.0;
    for tick in 0..15_000 {
        car.step(&c, &MANUAL, &g, &e, DT);
        if tick % 20 == 19 {
            let now = energy(&car);
            worst_gain = worst_gain.max(now - previous);
            previous = now;
        }
    }
    // Semi-implicit Euler conserves a slightly different energy than the
    // exact one, so allow a tiny rise between samples.
    assert!(
        worst_gain < 2e-5 * start,
        "energy rose by {worst_gain:.3} J between samples (start {start:.0} J)"
    );
    assert!(
        previous < 0.6 * start,
        "the slide should have dissipated energy"
    );
}

#[test]
fn an_undamped_frictionless_car_conserves_energy_while_bouncing() {
    // With every dissipative element removed the integrator should neither
    // create nor destroy energy beyond its small bounded error.
    let mut p = car_engine_manual(false);
    p.aero.drag_area_m2 = 0.0;
    p.aero.lift_area_front_m2 = 0.0;
    p.aero.lift_area_rear_m2 = 0.0;
    for axle in [&mut p.suspension.front, &mut p.suspension.rear] {
        axle.damper_bump_n_s_per_m = 0.0;
        axle.damper_rebound_n_s_per_m = 0.0;
    }
    let mut t = qts_tire::test_support::tire();
    t.vertical.damping_n_s_per_m = 0.0;
    let mut surface = tarmac();
    surface.friction_scale = 1e-9; // effectively frictionless
    t.vertical.rolling_resistance_coefficient = 0.0;
    let temp = t.thermal.optimal_temp_c;
    let mut car = qts_vehicle::Car::new(p, t.clone(), t, env().gravity, temp).unwrap();
    let g = qts_vehicle::FlatGround { surface };
    let e = env();
    let at_rest = car.mechanical_energy(&g, e.gravity) - car.drivetrain().kinetic_energy();
    // Small enough that the tires stay loaded: leaving and regaining
    // contact is a kink the integrator handles with larger error.
    car.state.pos.z += 0.01;
    car.state.ang_vel.x = 0.4;
    let start = car.mechanical_energy(&g, e.gravity) - car.drivetrain().kinetic_energy();
    let mut lo = start;
    let mut hi = start;
    let mut first_second = 0.0;
    let mut last_second = 0.0;
    for tick in 0..6000 {
        car.step(&DriverControls::default(), &MANUAL, &g, &e, DT);
        let now = car.mechanical_energy(&g, e.gravity) - car.drivetrain().kinetic_energy();
        lo = lo.min(now);
        hi = hi.max(now);
        if tick < 1000 {
            first_second += now / 1000.0;
        } else if tick >= 5000 {
            last_second += now / 1000.0;
        }
    }
    // Compare against the energy of the disturbance, not the large constant
    // gravitational term.
    let disturbance = start - at_rest;
    assert!(disturbance > 30.0, "disturbance {disturbance:.1} J");
    // Semi-implicit Euler conserves a nearby energy, so the exact energy
    // ripples by a few percent of the fastest mode's share but must not
    // drift.
    assert!(
        hi - lo < 0.08 * disturbance,
        "energy rippled by {:.2} J against a disturbance of {disturbance:.0} J",
        hi - lo
    );
    assert!(
        (last_second - first_second).abs() < 0.01 * disturbance,
        "energy drifted by {:.3} J against a disturbance of {disturbance:.0} J",
        last_second - first_second
    );
}

#[test]
fn electric_drive_energy_balances_against_the_battery() {
    let mut car = build(car_dual_motor());
    let g = ground();
    let e = env();
    let capacity = match &car.params().drivetrain {
        DrivetrainParams::DualMotorElectric(d) => d.battery.capacity_j,
        _ => unreachable!(),
    };
    let mech0 = car.mechanical_energy(&g, e.gravity);
    let soc0 = 0.8;
    run(&mut car, &controls(0.0, 1.0, 0.0), &AUTO, 5.0);
    let mech1 = car.mechanical_energy(&g, e.gravity);
    let drawn = (soc0 - car.output().drive.battery_soc) * capacity;
    let gained = mech1 - mech0;
    assert!(gained > 1.0e5);
    // The battery must supply at least what the car gained (losses are
    // positive) and not absurdly more.
    assert!(drawn > gained, "drawn {drawn:.0} J, gained {gained:.0} J");
    assert!(
        drawn < 1.6 * gained,
        "drawn {drawn:.0} J, gained {gained:.0} J"
    );

    // Coasting down on lift-off regeneration returns less than was lost.
    let soc_before = car.output().drive.battery_soc;
    run(&mut car, &controls(0.0, 0.0, 0.0), &AUTO, 5.0);
    let mech2 = car.mechanical_energy(&g, e.gravity);
    let recovered = (car.output().drive.battery_soc - soc_before) * capacity;
    assert!(recovered > 0.0);
    assert!(recovered < mech1 - mech2);
}

#[test]
fn rear_wheels_of_a_front_drive_car_receive_no_drive_torque() {
    let mut car = build(car_engine_manual(false));
    run(&mut car, &controls(0.0, 1.0, 0.0), &AUTO, 2.0);
    let w = car.output().wheels;
    assert!(w[FL].drive_torque > 100.0 && w[FR].drive_torque > 100.0);
    assert_eq!((w[RL].drive_torque, w[RR].drive_torque), (0.0, 0.0));
}
