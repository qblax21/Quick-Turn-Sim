// SPDX-License-Identifier: Apache-2.0
//! Steering lock and ratio against the published figures they come from
//! (D-083): the angle at the tire, and the turning circle when driven.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::math::Vec3;
use qts_core::step::Simulation;
use qts_sim::Session;

/// Published turning circle, curb to curb, m, per car (see
/// docs/VALIDATION_SOURCES.md).
const TURNING_CIRCLE_M: [(&str, f64); 2] = [("hatch_tern_gt", 10.7), ("sedan_halden_e4", 11.6)];

fn hold(s: &mut Session, steer: f64, throttle: f64, seconds: f64) {
    let frame = InputFrame::from_analog(steer, throttle, 0.0, 0.0, 0.0);
    for _ in 0..(seconds * 1000.0) as usize {
        s.step(&frame);
    }
}

fn front_mean_angle(s: &Session) -> f64 {
    let w = &s.car().output().wheels;
    0.5 * (w[0].steer_angle + w[1].steer_angle)
}

#[test]
fn the_steering_ratio_gives_the_expected_angle_at_the_tire() {
    for car in CARS {
        let mut s = session(car, "test_track");
        let st = s.car().params().steering.clone();
        let toe = s.car().params().suspension.front.static_toe_rad;
        assert!(toe.abs() < 1e-9, "{car}: test assumes no front toe");
        for degrees in [5.0_f64, 20.0, 45.0, 90.0] {
            let wheel = degrees.to_radians();
            hold(&mut s, wheel / st.max_wheel_angle_rad, 0.0, 0.3);
            // At rest there is no steering compliance to speak of; the
            // mean of the two fronts is the steering-wheel angle over the
            // ratio, Ackermann splitting it evenly at these angles.
            let expected = wheel / st.ratio;
            let got = front_mean_angle(&s);
            assert!(
                (got - expected).abs() < 0.01 * expected + 2e-5,
                "{car} at {degrees} deg: tire {:.4} deg, expected {:.4}",
                got.to_degrees(),
                expected.to_degrees()
            );
        }
        // Full lock: the steering wheel at its published lock.
        hold(&mut s, 1.0, 0.0, 0.3);
        let out = s.car().output();
        let (inner, outer) = (out.wheels[0].steer_angle, out.wheels[1].steer_angle);
        assert!(inner > outer, "{car}: Ackermann turns the inner wheel more");
        let p = s.car().params();
        let width = s.car().tire_params(0).geometry.width_m;
        let published = TURNING_CIRCLE_M
            .iter()
            .find(|(c, _)| *c == car)
            .map(|(_, d)| *d);
        if let Some(d) = published {
            // The car turns about the rear axle line at its mean front
            // angle; the outer front tire's outside edge then traces the
            // published curb-to-curb circle.
            let (l, t) = (p.body.wheelbase_m, p.suspension.front.track_m);
            let outer_r = 0.5 * d - 0.5 * width;
            let expected = (l / ((outer_r * outer_r - l * l).sqrt() - 0.5 * t)).atan();
            // Lock over ratio is that angle (the data are consistent)...
            let nominal = st.max_wheel_angle_rad / st.ratio;
            assert!(
                (nominal - expected).abs() < 0.05_f64.to_radians(),
                "{car}: lock over ratio {:.2} deg, from the turning circle {:.2}",
                nominal.to_degrees(),
                expected.to_degrees()
            );
            // ...and the tires average it, give or take the Ackermann
            // split, which is not symmetric at this much lock.
            let mean = 0.5 * (inner + outer);
            assert!(
                (mean - expected).abs() < 0.5_f64.to_radians(),
                "{car}: mean front angle at lock {:.2} deg, from the turning circle {:.2}",
                mean.to_degrees(),
                expected.to_degrees()
            );
        }
    }
}

#[test]
fn driven_at_full_lock_the_car_turns_its_published_circle() {
    for (car, published) in TURNING_CIRCLE_M {
        let mut s = session(car, "test_track");
        // Creep round at full lock and trace the outer front tire's outside
        // edge, which is what a curb-to-curb figure measures.
        hold(&mut s, 1.0, 0.0, 0.5);
        let width = s.car().tire_params(0).geometry.width_m;
        let mut points = Vec::new();
        for i in 0..40_000 {
            let speed = s.car().output().speed;
            let throttle = if speed < 2.0 { 0.15 } else { 0.0 };
            s.step(&InputFrame::from_analog(
                1.0,
                throttle,
                if speed > 2.5 { 0.2 } else { 0.0 },
                0.0,
                0.0,
            ));
            if i > 8000 && i % 20 == 0 {
                let car = s.car();
                let b = car.output().wheels[1].center_body;
                let c = Vec3::new(b[0], b[1], b[2]);
                let world = car.state.pos + car.state.rot.rotate(c);
                points.push([world.x, world.y]);
            }
        }
        let n = points.len() as f64;
        let cx = points.iter().map(|p| p[0]).sum::<f64>() / n;
        let cy = points.iter().map(|p| p[1]).sum::<f64>() / n;
        let r = points
            .iter()
            .map(|p| (p[0] - cx).hypot(p[1] - cy))
            .sum::<f64>()
            / n;
        let diameter = 2.0 * (r + 0.5 * width);
        assert!(
            (diameter - published).abs() < 0.04 * published,
            "{car}: turning circle {diameter:.2} m, published {published} m"
        );
        println!("{car}: turning circle {diameter:.2} m, published {published} m");
    }
}
