// SPDX-License-Identifier: Apache-2.0
//! Brake disc temperature, fade and cooling; tire wear at the car level.

mod common;

use common::*;
use qts_vehicle::car::{FL, RL};
use qts_vehicle::test_support::car_engine_manual;
use qts_vehicle::{Car, DriverControls};

/// Kinetic energy of the body and the spinning wheels, J.
fn kinetic(car: &Car) -> f64 {
    let p = car.params();
    let mass = p.body.sprung_mass_kg
        + 2.0 * p.suspension.front.unsprung_mass_kg
        + 2.0 * p.suspension.rear.unsprung_mass_kg;
    let v = car.state.vel.length();
    let spin: f64 = (0..4)
        .map(|i| {
            let j = if i < 2 {
                p.suspension.front.wheel_inertia_kg_m2
            } else {
                p.suspension.rear.wheel_inertia_kg_m2
            };
            0.5 * j * car.state.wheel_omega[i].powi(2)
        })
        .sum();
    0.5 * mass * v * v + spin
}

#[test]
fn every_joule_into_a_disc_is_stored_or_cooled_away() {
    let mut car = build(car_engine_manual(false));
    car.set_forward_speed(30.0);
    let (g, e) = (ground(), env());
    let start = car.state.brake_temp_c;
    let mut net = [0.0; 4];
    let c = controls(0.0, 0.0, 0.8);
    for tick in 0..20_000 {
        let c = if tick < 8000 {
            c
        } else {
            DriverControls::default()
        };
        car.step(&c, &MANUAL, &g, &e, DT);
        for (i, w) in car.output().wheels.iter().enumerate() {
            net[i] += (w.brake_heat_w - w.brake_cooling_w) * DT;
        }
    }
    let p = car.params();
    for i in 0..4 {
        let th = if i < 2 {
            &p.brakes.thermal_front
        } else {
            &p.brakes.thermal_rear
        };
        let stored = th.heat_capacity_j_per_k * (car.state.brake_temp_c[i] - start[i]);
        assert!(
            (stored - net[i]).abs() < 1e-6 * net[i].abs().max(1.0),
            "wheel {i}: stored {stored:.1} J, net in {:.1} J",
            net[i]
        );
    }
    assert!(car.state.brake_temp_c[FL] > start[FL] + 20.0);
}

#[test]
fn a_stop_puts_most_of_the_kinetic_energy_into_the_brakes() {
    let mut car = build(car_engine_manual(false));
    car.set_forward_speed(30.0);
    let (g, e) = (ground(), env());
    let before = kinetic(&car);
    let mut work = 0.0;
    let c = controls(0.0, 0.0, 1.0);
    while car.output().speed > 0.05 || work == 0.0 {
        car.step(&c, &MANUAL, &g, &e, DT);
        let p = car.params();
        for (i, w) in car.output().wheels.iter().enumerate() {
            let share = if i < 2 {
                p.brakes.thermal_front.disc_heat_share
            } else {
                p.brakes.thermal_rear.disc_heat_share
            };
            work += w.brake_heat_w / share * DT;
        }
    }
    let lost = before - kinetic(&car);
    // The rest goes into tire slip under anti-lock, rolling resistance
    // and drag.
    let share = work / lost;
    assert!(
        (0.75..0.98).contains(&share),
        "brakes took {:.0} kJ of {:.0} kJ ({:.0} %)",
        work / 1e3,
        lost / 1e3,
        share * 100.0
    );
}

#[test]
fn repeated_hard_stops_fade_the_brakes() {
    let mut car = build(car_engine_manual(false));
    let (g, e) = (ground(), env());
    let stop = |car: &mut Car| -> f64 {
        car.set_forward_speed(40.0);
        let c = controls(0.0, 0.0, 1.0);
        let mut ticks = 0;
        while car.output().speed > 10.0 || ticks == 0 {
            car.step(&c, &MANUAL, &g, &e, DT);
            ticks += 1;
        }
        // Mean deceleration from 40 to 10 m/s.
        30.0 / (f64::from(ticks) * DT)
    };
    let first = stop(&mut car);
    let mut last = first;
    for _ in 0..14 {
        last = stop(&mut car);
    }
    let out = car.output();
    let hot = out.wheels[FL].brake_temp_c;
    assert!(hot > 550.0, "front discs only reached {hot:.0} degC");
    assert!(out.wheels[FL].brake_friction_factor < 0.85);
    assert!(
        last < 0.9 * first,
        "deceleration {first:.2} m/s2 at first, {last:.2} after fade"
    );
    // Rear discs work less and stay cooler.
    assert!(out.wheels[RL].brake_temp_c < hot);
}

#[test]
fn discs_cool_faster_at_speed_and_with_a_duct() {
    let p = car_engine_manual(false);
    let mut th = p.brakes.thermal_front.clone();
    let at_rest = th.cooling_w(400.0, 20.0, 0.0);
    let moving = th.cooling_w(400.0, 20.0, 30.0);
    th.duct_opening = 1.0;
    let ducted = th.cooling_w(400.0, 20.0, 30.0);
    assert!(at_rest > 0.0 && moving > at_rest && ducted > moving);
    // Radiation matters when hot: more than linear in temperature.
    th.duct_opening = 0.0;
    assert!(th.cooling_w(600.0, 20.0, 0.0) > 1.6 * th.cooling_w(300.0, 20.0, 0.0));
    // Nothing flows at air temperature.
    assert!(th.cooling_w(20.0, 20.0, 30.0).abs() < 1e-9);

    // In the car: hot discs cool while it rolls on, faster than at rest.
    let cooled = |speed: f64| {
        let mut car = build(car_engine_manual(false));
        car.set_forward_speed(speed);
        car.state.brake_temp_c = [400.0; 4];
        let throttle = if speed > 0.0 { 0.3 } else { 0.0 };
        run(&mut car, &controls(0.0, throttle, 0.0), &AUTO, 20.0);
        car.state.brake_temp_c[FL]
    };
    let (rolling, parked) = (cooled(25.0), cooled(0.0));
    assert!(rolling < 380.0, "rolling: {rolling:.0} degC after 20 s");
    assert!(
        parked > rolling + 10.0,
        "parked {parked:.0}, rolling {rolling:.0}"
    );
}

#[test]
fn a_car_wears_its_tires_by_sliding_them() {
    let mut car = build(car_engine_manual(false));
    car.set_forward_speed(20.0);
    // Gentle straight running barely wears anything.
    run(&mut car, &controls(0.0, 0.15, 0.0), &AUTO, 5.0);
    let gentle = car.output().wheels[FL].tire_wear / 5.0;
    // Hard cornering and braking wear the fronts much faster.
    run(&mut car, &controls(0.5, 0.6, 0.0), &AUTO, 5.0);
    run(&mut car, &controls(0.3, 0.0, 1.0), &AUTO, 3.0);
    let hard = (car.output().wheels[FL].tire_wear - 5.0 * gentle) / 8.0;
    assert!(gentle > 0.0, "{gentle}");
    assert!(
        hard > 3.0 * gentle,
        "per second: gentle {gentle:e}, hard {hard:e}"
    );
    // Placing the car again fits new tires and cold brakes.
    car.place(0.0, 0.0, 0.0, 0.0);
    assert_eq!(car.output().wheels[FL].tire_wear, 0.0);
}
