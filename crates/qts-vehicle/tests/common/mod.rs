// SPDX-License-Identifier: Apache-2.0
//! Shared helpers for the vehicle integration tests.
#![allow(dead_code)]

use qts_tire::test_support::{tarmac, tire};
use qts_vehicle::params::CarParams;
use qts_vehicle::{Assists, Car, DriverControls, Environment, FlatGround};

pub const DT: f64 = 0.001;

pub fn ground() -> FlatGround {
    FlatGround { surface: tarmac() }
}

pub fn env() -> Environment {
    Environment::default()
}

/// Builds a car on the test tire with tires at their optimum temperature.
pub fn build(params: CarParams) -> Car {
    let t = tire();
    let temp = t.thermal.optimal_temp_c;
    Car::new(params, t.clone(), t, env().gravity, temp).expect("valid test car")
}

pub const AUTO: Assists = Assists {
    auto_clutch: true,
    auto_shift: true,
    traction_control: true,
};

pub const MANUAL: Assists = Assists {
    auto_clutch: false,
    auto_shift: false,
    traction_control: true,
};

pub fn controls(steer: f64, throttle: f64, brake: f64) -> DriverControls {
    DriverControls {
        steer,
        throttle,
        brake,
        ..DriverControls::default()
    }
}

/// Steps `car` for `seconds` with fixed controls on flat tarmac.
pub fn run(car: &mut Car, c: &DriverControls, assists: &Assists, seconds: f64) {
    let g = ground();
    let e = env();
    let ticks = (seconds / DT).round() as usize;
    for _ in 0..ticks {
        car.step(c, assists, &g, &e, DT);
    }
}

/// Height of the centre of gravity of the whole car (sprung plus unsprung)
/// above the ground at rest.
pub fn total_cg_height(car: &Car) -> f64 {
    let p = car.params();
    let mut moment = p.body.sprung_mass_kg * p.body.cg_height_m;
    for (i, w) in car.output().wheels.iter().enumerate() {
        let axle = if i < 2 {
            &p.suspension.front
        } else {
            &p.suspension.rear
        };
        let height = p.body.cg_height_m + w.center_body[2];
        moment += axle.unsprung_mass_kg * height;
    }
    moment / p.total_mass_kg()
}
