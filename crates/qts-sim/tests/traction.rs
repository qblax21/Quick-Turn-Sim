// SPDX-License-Identifier: Apache-2.0
//! Traction control: a floored launch must not turn into sustained
//! wheelspin, and the car must still get away quickly.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::step::Simulation;

fn worst_drive_slip(session: &qts_sim::Session) -> f64 {
    session
        .car()
        .output()
        .wheels
        .iter()
        .map(|w| w.slip_ratio)
        .fold(0.0, f64::max)
}

#[test]
fn a_floored_launch_is_held_near_the_target_slip() {
    for car in CARS {
        let mut session = session(car, "test_track");
        assert!(
            session.car().params().traction_control.enabled,
            "{car} is expected to have traction control"
        );
        let rate = f64::from(session.tick_rate_hz());
        let floored = InputFrame::from_analog(0.0, 1.0, 0.0, 0.0, 0.0);
        // Seconds spent above twice the target slip once the launch has
        // settled (an automatic clutch take-up spins the wheels briefly
        // whatever the accelerator does), and the speed reached.
        let mut spinning = 0.0;
        let seconds = 5.0;
        for tick in 0..(seconds * rate) as u64 {
            session.step(&floored);
            let t = tick as f64 / rate;
            let limit = 2.0 * session.car().params().traction_control.slip_threshold;
            if t > 1.0 && worst_drive_slip(&session) > limit {
                spinning += 1.0 / rate;
            }
        }
        let speed = session.car().output().vel_body[0];
        assert!(
            spinning < 0.25,
            "{car}: {spinning:.2} s of wheelspin with the accelerator floored"
        );
        assert!(speed > 20.0, "{car}: only {speed:.1} m/s after {seconds} s");
    }
}
