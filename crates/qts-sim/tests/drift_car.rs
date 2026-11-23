// SPDX-License-Identifier: Apache-2.0
//! The Kestrel RC drift build (D-111): its angle kit gives about 60
//! degrees of lock, its power and locked differential break the rear tires
//! loose on throttle, and the slide can be held with opposite lock.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::step::Simulation;
use qts_sim::session::{Session, SessionConfig};
use qts_vehicle::Assists;

const CAR: &str = "coupe_kestrel_rc";

fn manual_session(car: &str) -> Session {
    let config = SessionConfig {
        assists: Assists {
            auto_clutch: true,
            auto_shift: false,
            traction_control: false,
            engine_damage: false,
        },
        ..SessionConfig::default()
    };
    Session::from_data(&data(), car, "test_track", config).unwrap()
}

fn body_slip_deg(s: &Session) -> f64 {
    let v = s.car().output().vel_body;
    v[1].atan2(v[0].abs().max(0.5)).to_degrees()
}

#[test]
fn the_angle_kit_gives_about_sixty_degrees_of_lock() {
    let mut s = manual_session(CAR);
    for _ in 0..1500 {
        s.step(&InputFrame::from_analog(1.0, 0.0, 0.0, 0.0, 0.0));
    }
    let o = s.car().output();
    let inner = o.wheels[0]
        .steer_angle
        .abs()
        .max(o.wheels[1].steer_angle.abs())
        .to_degrees();
    let outer = o.wheels[0]
        .steer_angle
        .abs()
        .min(o.wheels[1].steer_angle.abs())
        .to_degrees();
    eprintln!("full lock: inner {inner:.1} deg, outer {outer:.1} deg");
    assert!((55.0..=65.0).contains(&inner), "inner wheel {inner:.1} deg");
    // Little Ackermann: the outer wheel turns nearly as far as the inner.
    assert!(
        outer > 0.8 * inner,
        "outer {outer:.1} against inner {inner:.1}"
    );
}

/// Rolls at `speed` in second gear, turns in and floors it for half a
/// second, then holds the slide with a simple driver: opposite lock in
/// proportion to the slide and to the rotation, and throttle to keep the
/// rear sliding. Returns the largest body slip angle, and the least and
/// largest while holding (from 0.3 to 2.0 s), degrees.
fn power_slide(car: &str, speed: f64) -> (f64, f64, f64) {
    let mut s = manual_session(car);
    let (x, y, h) = s.track().spawn();
    s.car_mut().place(x, y, h, 0.0);
    s.car_mut().set_forward_speed(speed);
    let mut second = InputFrame::from_analog(0.0, 0.2, 0.0, 0.0, 0.0);
    second.gear_request = 2;
    for _ in 0..800 {
        s.step(&second);
    }
    let max = s.car().params().steering.max_wheel_angle_rad;
    let ratio = s.car().params().steering.ratio;
    let wheel = move |deg: f64| (deg.to_radians() * ratio / max).clamp(-1.0, 1.0);
    let mut peak: f64 = 0.0;
    // Turn in left and floor it.
    for _ in 0..500 {
        let mut f = InputFrame::from_analog(wheel(15.0), 1.0, 0.0, 0.0, 0.0);
        f.gear_request = 2;
        s.step(&f);
        peak = peak.max(body_slip_deg(&s).abs());
    }
    // Hold it. In a left-hand slide the body slip is negative (the car
    // moves to the right of where it points) and opposite lock is to the
    // right, the same sign.
    let (mut least, mut most) = (f64::MAX, 0.0_f64);
    for t in 0..2000 {
        let slip = body_slip_deg(&s);
        let yaw = s.car().output().yaw_rate;
        let counter = wheel((0.5 * slip - 7.0 * yaw).clamp(-55.0, 55.0));
        let throttle = (0.6 + 0.03 * (25.0 - slip.abs())).clamp(0.1, 1.0);
        let mut f = InputFrame::from_analog(counter, throttle, 0.0, 0.0, 0.0);
        f.gear_request = 2;
        s.step(&f);
        peak = peak.max(slip.abs());
        if t >= 300 {
            least = least.min(slip.abs());
            most = most.max(slip.abs());
        }
    }
    (peak, least, most)
}

#[test]
fn power_breaks_the_rear_loose_and_the_slide_can_be_held() {
    let (peak, least, most) = power_slide(CAR, 14.0);
    eprintln!("{CAR}: body slip peak {peak:.1} deg; held between {least:.1} and {most:.1} deg");
    assert!(peak > 20.0, "the rear did not come round: {peak:.1} deg");
    assert!(
        least > 10.0 && most < 40.0,
        "slide not held: {least:.1} to {most:.1} deg"
    );
    // The front-drive hatch does nothing of the kind on the same inputs.
    let (hatch_peak, _, _) = power_slide("hatch_tern_gt", 14.0);
    eprintln!("hatch_tern_gt: body slip peak {hatch_peak:.1} deg");
    assert!(hatch_peak < 10.0, "the hatch slid {hatch_peak:.1} deg");
}
