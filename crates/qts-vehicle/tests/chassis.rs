// SPDX-License-Identifier: Apache-2.0
//! Chassis, suspension, steering, brakes and aerodynamics.

mod common;

use common::*;
use qts_core::math::Vec3;
use qts_core::snapshot::{restore_bytes, snapshot_bytes, state_hash};
use qts_vehicle::car::{FL, FR, RL, RR};
use qts_vehicle::test_support::{car_dual_motor, car_engine_manual};
use qts_vehicle::DriverControls;

fn no_aero(mut p: qts_vehicle::CarParams) -> qts_vehicle::CarParams {
    p.aero.drag_area_m2 = 0.0;
    p.aero.lift_area_front_m2 = 0.0;
    p.aero.lift_area_rear_m2 = 0.0;
    p
}

#[test]
fn a_parked_car_stays_put_at_its_design_ride_height() {
    for params in [car_engine_manual(false), car_dual_motor()] {
        let mut car = build(params);
        let start = car.state.pos;
        run(&mut car, &DriverControls::default(), &MANUAL, 5.0);
        let moved = car.state.pos - start;
        assert!(moved.length() < 0.003, "moved {moved:?}");
        assert!(car.state.vel.length() < 1e-3);
        assert!(car.state.ang_vel.length() < 1e-3);
        for travel in car.state.susp {
            assert!(travel.abs() < 0.003, "suspension travel {travel}");
        }
    }
}

#[test]
fn static_tire_loads_carry_the_weight_in_the_right_proportion() {
    let mut car = build(car_engine_manual(false));
    run(&mut car, &DriverControls::default(), &MANUAL, 3.0);
    let p = car.params().clone();
    let g = env().gravity;
    let w = car.output().wheels;
    let total: f64 = w.iter().map(|x| x.load).sum();
    assert!((total - p.total_mass_kg() * g).abs() < 0.002 * total);
    assert!((w[FL].load - w[FR].load).abs() < 1.0);
    assert!((w[RL].load - w[RR].load).abs() < 1.0);
    let front_expected = p.body.sprung_mass_kg * g * p.cg_to_rear_axle_m() / p.body.wheelbase_m
        + 2.0 * p.suspension.front.unsprung_mass_kg * g;
    assert!((w[FL].load + w[FR].load - front_expected).abs() < 0.003 * front_expected);
}

#[test]
fn braking_moves_load_forward_by_mass_times_decel_times_height_over_wheelbase() {
    let mut car = build(no_aero(car_engine_manual(false)));
    run(&mut car, &DriverControls::default(), &MANUAL, 1.0);
    let static_front = car.output().wheels[FL].load + car.output().wheels[FR].load;
    let h = total_cg_height(&car);
    car.set_forward_speed(30.0);
    run(&mut car, &controls(0.0, 0.0, 0.35), &MANUAL, 1.5);
    let out = *car.output();
    let decel = -out.accel_body[0];
    assert!(decel > 2.0, "decel {decel}");
    let front = out.wheels[FL].load + out.wheels[FR].load;
    let p = car.params();
    let expected = p.total_mass_kg() * decel * h / p.body.wheelbase_m;
    let got = front - static_front;
    assert!(
        (got - expected).abs() < 0.05 * expected,
        "front load gain {got:.0} N, expected {expected:.0} N"
    );
    assert!(car.state.rot.pitch() < 0.0, "nose should dive");
}

#[test]
fn cornering_moves_load_outward_and_rolls_the_body_outward() {
    let mut car = build(no_aero(car_engine_manual(false)));
    let h = total_cg_height(&car);
    car.set_forward_speed(15.0);
    // A gentle left turn held by a little throttle through the auto clutch.
    let mut c = controls(0.05, 0.0, 0.0);
    c.gear_request = Some(3);
    c.throttle = 0.15;
    run(&mut car, &c, &AUTO, 6.0);
    let out = *car.output();
    let ay = out.accel_body[1];
    assert!(out.yaw_rate > 0.0, "left steer must yaw left");
    assert!(ay > 1.0, "lateral acceleration {ay}");
    let left = out.wheels[FL].load + out.wheels[RL].load;
    let right = out.wheels[FR].load + out.wheels[RR].load;
    let p = car.params();
    let track = p.suspension.front.track_m;
    let expected = p.total_mass_kg() * ay * h / track;
    let got = 0.5 * (right - left);
    assert!(
        (got - expected).abs() < 0.06 * expected,
        "side load transfer {got:.0} N, expected {expected:.0} N"
    );
    assert!(
        car.state.rot.roll() > 0.0,
        "left side should rise in a left turn"
    );
}

#[test]
fn stiffer_front_bar_takes_a_larger_share_of_the_load_transfer() {
    let run_with = |front_bar: f64| {
        let mut p = no_aero(car_engine_manual(false));
        p.suspension.front.anti_roll_bar_n_per_m = front_bar;
        let mut car = build(p);
        car.set_forward_speed(15.0);
        let mut c = controls(0.05, 0.15, 0.0);
        c.gear_request = Some(3);
        run(&mut car, &c, &AUTO, 6.0);
        let w = car.output().wheels;
        (w[FR].load - w[FL].load) / (w[FR].load - w[FL].load + w[RR].load - w[RL].load)
    };
    assert!(run_with(30_000.0) > run_with(2000.0) + 0.05);
}

#[test]
fn suspension_geometry_resists_roll_and_dive() {
    // A higher roll centre carries more of the lateral force through the
    // links, so the body rolls less for the same cornering.
    let roll_with = |height: f64| {
        let mut p = no_aero(car_engine_manual(false));
        p.suspension.front.roll_center_height_m = height;
        p.suspension.rear.roll_center_height_m = height;
        let mut car = build(p);
        car.set_forward_speed(15.0);
        let mut c = controls(0.05, 0.15, 0.0);
        c.gear_request = Some(3);
        run(&mut car, &c, &AUTO, 6.0);
        car.state.rot.roll() / car.output().accel_body[1]
    };
    let low = roll_with(0.0);
    let high = roll_with(0.2);
    assert!(low > 0.0 && high > 0.0);
    assert!(
        high < 0.8 * low,
        "roll per lateral g: {low:.5} low, {high:.5} high"
    );

    // Anti-dive geometry reduces nose dive under the same braking.
    let dive_with = |anti: f64| {
        let mut p = no_aero(car_engine_manual(false));
        p.suspension.front.anti_pitch_tan = anti;
        p.suspension.rear.anti_pitch_tan = anti;
        let mut car = build(p);
        car.set_forward_speed(30.0);
        run(&mut car, &controls(0.0, 0.0, 0.35), &MANUAL, 1.5);
        -car.state.rot.pitch() / -car.output().accel_body[0]
    };
    let plain = dive_with(0.0);
    let anti = dive_with(0.15);
    assert!(plain > 0.0);
    assert!(
        anti < 0.8 * plain,
        "dive per decel: {plain:.5} plain, {anti:.5} with anti-dive"
    );
}

#[test]
fn steering_torque_pushes_back_toward_centre() {
    let mut car = build(car_engine_manual(false));
    car.set_forward_speed(20.0);
    let mut c = controls(0.04, 0.2, 0.0);
    c.gear_request = Some(3);
    run(&mut car, &c, &AUTO, 4.0);
    let left_turn = car.output().steering_torque_nm;
    assert!(left_turn < -0.5, "torque in a left turn: {left_turn}");
    assert!(left_turn > -60.0, "implausibly large: {left_turn}");

    let mut car = build(car_engine_manual(false));
    car.set_forward_speed(20.0);
    c.steer = -0.04;
    run(&mut car, &c, &AUTO, 4.0);
    let right_turn = car.output().steering_torque_nm;
    assert!(
        (left_turn + right_turn).abs() < 0.05 * left_turn.abs(),
        "should mirror"
    );
}

#[test]
fn ackermann_turns_the_inside_wheel_more() {
    let mut car = build(car_engine_manual(false));
    run(&mut car, &controls(0.5, 0.0, 0.0), &MANUAL, 0.01);
    let w = car.output().wheels;
    assert!(w[FL].steer_angle > w[FR].steer_angle);
    assert!(w[FR].steer_angle > 0.0);
    assert_eq!(w[RL].steer_angle, 0.0);
}

#[test]
fn anti_lock_keeps_the_wheels_turning_and_does_not_lengthen_the_stop() {
    let stop = |abs: bool| {
        let mut p = car_engine_manual(false);
        p.brakes.abs.enabled = abs;
        let mut car = build(p);
        car.set_forward_speed(27.0);
        let start = car.state.pos.x;
        let g = ground();
        let e = env();
        let c = controls(0.0, 0.0, 1.0);
        let mut min_ratio: f64 = 1.0;
        let mut ticks = 0;
        while car.output().speed > 0.5 || ticks < 10 {
            car.step(&c, &MANUAL, &g, &e, DT);
            ticks += 1;
            let out = car.output();
            if out.speed > 8.0 && ticks > 300 {
                for w in &out.wheels[..2] {
                    min_ratio = min_ratio.min(w.omega * w.rolling_radius / out.speed);
                }
            }
            assert!(ticks < 20_000, "car never stopped");
        }
        (car.state.pos.x - start, min_ratio)
    };
    let (with_abs, ratio_abs) = stop(true);
    let (without_abs, ratio_locked) = stop(false);
    assert!(
        ratio_abs > 0.5,
        "front wheels locked with ABS on: {ratio_abs}"
    );
    assert!(ratio_locked < 0.05, "front wheels should lock with ABS off");
    assert!(
        with_abs <= without_abs * 1.02,
        "ABS {with_abs:.1} m, locked {without_abs:.1} m"
    );
}

#[test]
fn handbrake_locks_only_the_rear_wheels() {
    let mut car = build(car_engine_manual(false));
    car.set_forward_speed(20.0);
    let c = DriverControls {
        handbrake: 1.0,
        ..DriverControls::default()
    };
    run(&mut car, &c, &MANUAL, 0.5);
    let w = car.output().wheels;
    assert!(w[RL].omega.abs() < 0.5 && w[RR].omega.abs() < 0.5);
    assert!(w[FL].omega > 30.0 && w[FR].omega > 30.0);
}

#[test]
fn drag_matches_half_rho_cda_v_squared() {
    let mut car = build(car_engine_manual(false));
    car.set_forward_speed(40.0);
    run(&mut car, &DriverControls::default(), &MANUAL, 0.002);
    let p = car.params();
    let expected = 0.5 * p.aero.air_density_kg_per_m3 * p.aero.drag_area_m2 * 40.0 * 40.0;
    let got = car.output().aero_drag_n;
    assert!(
        (got - expected).abs() < 0.01 * expected,
        "{got} vs {expected}"
    );
}

#[test]
fn lift_changes_axle_loads_at_speed() {
    let loads_at = |lift: f64| {
        let mut p = car_engine_manual(false);
        p.aero.drag_area_m2 = 0.0;
        p.aero.lift_area_front_m2 = lift;
        p.aero.lift_area_rear_m2 = lift;
        let mut car = build(p);
        car.set_forward_speed(50.0);
        let c = DriverControls {
            gear_request: Some(0),
            ..DriverControls::default()
        };
        run(&mut car, &c, &MANUAL, 1.5);
        let v = car.output().vel_body[0];
        let total: f64 = car.output().wheels.iter().map(|w| w.load).sum();
        (total, v)
    };
    let (neutral, _) = loads_at(0.0);
    let (lifting, v) = loads_at(0.2);
    let expected = 2.0 * 0.5 * 1.2 * 0.2 * v * v;
    assert!(
        (neutral - lifting - expected).abs() < 0.05 * expected,
        "load lost {} N, expected {expected} N",
        neutral - lifting
    );
}

#[test]
fn upside_down_car_rests_on_its_roof_without_blowing_up() {
    let mut car = build(car_engine_manual(false));
    car.state.pos.z = 1.6;
    car.state.rot = qts_core::math::Quat::from_axis_angle(Vec3::X, std::f64::consts::PI);
    run(&mut car, &DriverControls::default(), &MANUAL, 4.0);
    assert!(car.is_finite());
    assert!(car.output().body_contacts >= 2);
    assert!(
        car.state.pos.z > 0.2 && car.state.pos.z < 1.5,
        "z={}",
        car.state.pos.z
    );
    assert!(car.state.vel.length() < 0.2);
}

#[test]
fn snapshot_restores_the_exact_trajectory() {
    for params in [car_engine_manual(true), car_dual_motor()] {
        let mut car = build(params.clone());
        let g = ground();
        let e = env();
        let input = |i: usize| {
            let t = i as f64 * DT;
            let mut c = controls((t * 1.3).sin() * 0.3, 0.8, 0.0);
            if t > 3.0 {
                c.throttle = 0.0;
                c.brake = 0.7;
            }
            c
        };
        for i in 0..2500 {
            car.step(&input(i), &AUTO, &g, &e, DT);
        }
        let bytes = snapshot_bytes(&car);
        for i in 2500..5000 {
            car.step(&input(i), &AUTO, &g, &e, DT);
        }
        let mut other = build(params);
        restore_bytes(&mut other, &bytes).unwrap();
        for i in 2500..5000 {
            other.step(&input(i), &AUTO, &g, &e, DT);
        }
        assert_eq!(state_hash(&car), state_hash(&other));
        assert_eq!(car.state, other.state);
    }
}

#[test]
fn identical_inputs_give_identical_hashes_every_tick() {
    let mut a = build(car_dual_motor());
    let mut b = build(car_dual_motor());
    let g = ground();
    let e = env();
    for i in 0..4000 {
        let t = i as f64 * DT;
        let c = controls((t * 2.0).sin() * 0.5, (t * 0.7).cos().abs(), 0.0);
        a.step(&c, &AUTO, &g, &e, DT);
        b.step(&c, &AUTO, &g, &e, DT);
        assert_eq!(state_hash(&a), state_hash(&b), "diverged at tick {i}");
    }
}

/// A tilted plane rising along +x and +y.
struct Slope {
    surface: qts_tire::SurfaceParams,
    grade_x: f64,
    grade_y: f64,
}

impl qts_vehicle::Ground for Slope {
    fn sample(&self, x: f64, y: f64) -> qts_vehicle::GroundSample<'_> {
        qts_vehicle::GroundSample {
            height: self.grade_x * x + self.grade_y * y,
            normal: Vec3::new(-self.grade_x, -self.grade_y, 1.0).normalized_or(Vec3::Z),
            surface: &self.surface,
            surface_index: 0,
        }
    }
}

fn run_on(car: &mut qts_vehicle::Car, slope: &Slope, c: &DriverControls, seconds: f64) {
    let e = env();
    for _ in 0..(seconds / DT).round() as usize {
        car.step(c, &MANUAL, slope, &e, DT);
    }
}

#[test]
fn a_car_in_neutral_rolls_back_down_a_grade_at_about_g_sin_theta() {
    let slope = Slope {
        surface: qts_tire::test_support::tarmac(),
        grade_x: 0.10,
        grade_y: 0.0,
    };
    let mut car = build(no_aero(car_engine_manual(false)));
    // Hold it on the brake while it settles, then let go.
    run_on(&mut car, &slope, &controls(0.0, 0.0, 1.0), 2.0);
    assert!(
        car.state.vel.length() < 0.02,
        "crept at {:?}",
        car.state.vel
    );
    let total: f64 = car.output().wheels.iter().map(|w| w.load).sum();
    let theta = (0.10f64).atan();
    let weight = car.params().total_mass_kg() * env().gravity;
    assert!(
        (total - weight * theta.cos()).abs() < 0.01 * weight,
        "tire loads {total} against {}",
        weight * theta.cos()
    );
    run_on(&mut car, &slope, &DriverControls::default(), 1.0);
    let v1 = car.state.vel;
    run_on(&mut car, &slope, &DriverControls::default(), 2.0);
    let v2 = car.state.vel;
    // Speed gained along the slope, downhill being -x.
    let gained = ((v2 - v1).length()) / 2.0;
    let ideal = env().gravity * theta.sin();
    assert!(v2.x < v1.x && v2.z < v1.z, "should roll back downhill");
    assert!(
        gained > 0.80 * ideal && gained < 1.0 * ideal,
        "acceleration {gained} against g sin(theta) = {ideal}"
    );
    // The body sits parallel to the slope, nose up.
    assert!((car.state.rot.pitch().abs() - theta).abs() < 0.01);
}

#[test]
fn a_braked_car_holds_on_a_cross_slope() {
    let slope = Slope {
        surface: qts_tire::test_support::tarmac(),
        grade_x: 0.0,
        grade_y: 0.08,
    };
    let mut car = build(car_engine_manual(false));
    run_on(&mut car, &slope, &controls(0.0, 0.0, 1.0), 2.0);
    let start = car.state.pos;
    run_on(&mut car, &slope, &controls(0.0, 0.0, 1.0), 3.0);
    let moved = car.state.pos - start;
    assert!(moved.length() < 0.05, "slid {moved:?}");
    // The downhill (right-hand) tires carry more.
    let w = car.output().wheels;
    assert!(w[FR].load > w[FL].load && w[RR].load > w[RL].load);
}
