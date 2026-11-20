// SPDX-License-Identifier: Apache-2.0
//! The steering system: speed-sensitive assist, rack friction and inertia,
//! gyroscopic and weight moments at the kingpins.

mod common;

use common::*;
use qts_vehicle::car::{FL, FR};
use qts_vehicle::test_support::car_engine_manual;
use qts_vehicle::{CarParams, DriverControls};

fn with_steering(f: impl FnOnce(&mut CarParams)) -> CarParams {
    let mut p = car_engine_manual(false);
    f(&mut p);
    p
}

fn epas(p: &mut CarParams) {
    let s = &mut p.steering;
    s.assist_speed_kmh = vec![0.0, 50.0, 150.0];
    s.assist_gain = vec![6.0, 2.0, 1.0];
    s.assist_threshold_nm = 0.8;
}

#[test]
fn the_assist_falls_with_speed_and_leaves_the_centre_alone() {
    let p = with_steering(epas);
    let s = &p.steering;
    // Below the threshold the driver carries the whole load.
    assert_eq!(s.driver_torque(0.5, 0.0), 0.5);
    assert_eq!(s.driver_torque(-0.8, 40.0), -0.8);
    // Continuous at the threshold, odd in the load.
    let just = s.driver_torque(0.800_001, 0.0);
    assert!((just - 0.8).abs() < 1e-6);
    assert_eq!(s.driver_torque(-10.0, 20.0), -s.driver_torque(10.0, 20.0));
    // The same load is lighter when parking than at speed.
    let parking = s.driver_torque(20.0, 0.0);
    let town = s.driver_torque(20.0, 50.0 / 3.6);
    let motorway = s.driver_torque(20.0, 150.0 / 3.6);
    assert!((parking - (20.0 + 6.0 * 0.8) / 7.0).abs() < 1e-12);
    assert!(
        parking < town && town < motorway,
        "{parking} {town} {motorway}"
    );
    // Interpolated between table speeds and held beyond the ends.
    assert!((s.assist_gain_at(100.0 / 3.6) - 1.5).abs() < 1e-12);
    assert_eq!(s.assist_gain_at(300.0 / 3.6), 1.0);
    // The driver never feels more than the load, or the wrong sign.
    for load in [-30.0, -3.0, -0.1, 0.0, 0.1, 3.0, 30.0] {
        let d = s.driver_torque(load, 10.0);
        assert!(d.abs() <= load.abs() + 1e-12 && d * load >= 0.0);
    }
}

#[test]
fn in_a_corner_the_assist_carries_part_of_the_load() {
    let mut car = build(with_steering(epas));
    car.set_forward_speed(15.0);
    run(&mut car, &controls(0.08, 0.3, 0.0), &AUTO, 3.0);
    let s = car.output().steering;
    assert!(s.load_nm.abs() > 2.0, "load {}", s.load_nm);
    assert!(s.assist_nm * s.load_nm < 0.0, "assist opposes the load");
    let felt = car.output().steering_torque_nm - s.friction_nm - s.inertia_nm;
    assert!(felt.abs() < s.load_nm.abs() && felt * s.load_nm > 0.0);
    let expected = car
        .params()
        .steering
        .assist_gain_at(car.output().vel_body[0]);
    assert!((s.assist_gain - expected).abs() < 1e-9 && s.assist_gain > 1.0);
}

#[test]
fn rack_friction_opposes_the_direction_the_wheel_is_turned() {
    let mut car = build(with_steering(|p| p.steering.rack_friction_nm = 1.2));
    let (g, e) = (ground(), env());
    let max = car.params().steering.max_wheel_angle_rad;
    let rate = 2.0; // rad/s at the steering wheel
    let mut c = DriverControls::default();
    run(&mut car, &c, &MANUAL, 0.5);
    for i in 0..400 {
        c.steer = rate * f64::from(i) * DT / max;
        car.step(&c, &MANUAL, &g, &e, DT);
    }
    let left = car.output().steering.friction_nm;
    assert!((left + 1.2).abs() < 0.05, "turning left: {left}");
    let start = c.steer;
    for i in 0..400 {
        c.steer = start - rate * f64::from(i) * DT / max;
        car.step(&c, &MANUAL, &g, &e, DT);
    }
    let right = car.output().steering.friction_nm;
    assert!((right - 1.2).abs() < 0.05, "turning right: {right}");
    // Held still, no friction torque.
    run(&mut car, &c, &MANUAL, 0.5);
    assert!(car.output().steering.friction_nm.abs() < 0.01);
}

#[test]
fn rack_inertia_resists_the_column_accelerating() {
    let j = 0.01;
    let mut car = build(with_steering(|p| p.steering.steering_inertia_kg_m2 = j));
    let (g, e) = (ground(), env());
    let max = car.params().steering.max_wheel_angle_rad;
    let accel = 20.0; // rad/s² at the steering wheel
    let mut c = DriverControls::default();
    run(&mut car, &c, &MANUAL, 0.3);
    for i in 0..300 {
        let t = f64::from(i) * DT;
        c.steer = 0.5 * accel * t * t / max;
        car.step(&c, &MANUAL, &g, &e, DT);
    }
    let torque = car.output().steering.inertia_nm;
    assert!((torque + j * accel).abs() < 0.02 * j * accel, "{torque}");
}

#[test]
fn spinning_front_wheels_add_a_centring_gyroscopic_moment_in_a_turn() {
    // No gyroscopic moment at rest.
    let mut car = build(car_engine_manual(false));
    run(&mut car, &controls(0.1, 0.0, 0.0), &MANUAL, 0.5);
    assert!(car.output().steering.gyro_nm.abs() < 1e-6);

    // Steady circle at speed: the yaw rate turns the wheels' angular
    // momentum, and with positive caster that pushes the steering back
    // toward the centre, by about 2·I·ω·r·sin(caster) at the kingpins.
    let mut car = build(car_engine_manual(false));
    car.set_forward_speed(25.0);
    run(&mut car, &controls(0.04, 0.35, 0.0), &AUTO, 4.0);
    let out = car.output();
    assert!(out.yaw_rate > 0.1, "yaw rate {}", out.yaw_rate);
    let p = car.params();
    let inertia = p.suspension.front.wheel_inertia_kg_m2;
    let spin = 0.5 * (car.state.wheel_omega[FL] + car.state.wheel_omega[FR]);
    let expected =
        -2.0 * inertia * spin * out.yaw_rate * p.steering.caster_rad.sin() / p.steering.ratio;
    let gyro = out.steering.gyro_nm;
    assert!(gyro < 0.0, "should centre a left turn: {gyro}");
    assert!(
        (gyro - expected).abs() < 0.25 * expected.abs(),
        "gyro {gyro:.3} N.m, expected about {expected:.3}"
    );
}

#[test]
fn weight_through_kingpin_inclination_centres_the_steering() {
    let mut car = build(car_engine_manual(false));
    car.set_forward_speed(1.0);
    run(&mut car, &controls(0.3, 0.05, 0.0), &AUTO, 1.0);
    let out = car.output();
    let s = out.steering;
    assert!(
        s.weight_nm < 0.0,
        "left steer, weight moment {}",
        s.weight_nm
    );
    let p = car.params();
    let d = 0.5 * (out.wheels[FL].steer_angle + out.wheels[FR].steer_angle);
    let fz = out.wheels[FL].load + out.wheels[FR].load;
    let kpi = -fz * p.steering.scrub_radius_m * p.steering.kingpin_inclination_rad.sin() * d.sin()
        / p.steering.ratio;
    // The caster part only sees the small left-right load difference here.
    assert!(
        (s.weight_nm - kpi).abs() < 0.3 * kpi.abs(),
        "{} vs {kpi}",
        s.weight_nm
    );
}
