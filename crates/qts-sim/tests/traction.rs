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
        // The drift build has none, by design (D-111); every road car does.
        if !session.car().params().traction_control.enabled {
            assert_eq!(car, "coupe_kestrel_rc", "{car} is expected to have traction control");
            continue;
        }
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

#[test]
fn reversing_back_over_the_line_does_not_set_a_lap() {
    let mut session = session("sedan_halden_e4", "ridgeline_circuit");
    let rate = f64::from(session.tick_rate_hz());
    let run = |session: &mut qts_sim::Session, frame: InputFrame, seconds: f64| {
        for _ in 0..(seconds * rate) as u64 {
            session.step(&frame);
        }
    };
    // Forward over the line, stop, then back over it in reverse.
    run(
        &mut session,
        InputFrame::from_analog(0.0, 0.5, 0.0, 0.0, 0.0),
        5.0,
    );
    assert!(session.timing().running, "the run starts at the line");
    run(
        &mut session,
        InputFrame::from_analog(0.0, 0.0, 1.0, 0.0, 0.0),
        5.0,
    );
    let stopped_at = session.car().state.pos.x;
    run(
        &mut session,
        InputFrame::from_analog(0.0, 0.5, 0.0, 0.0, 0.0).with_gear(-1),
        9.0,
    );
    let backed_to = session.car().state.pos.x;
    assert!(
        stopped_at > 35.0 && backed_to < 25.0,
        "the car should cross the line both ways (stopped at {stopped_at:.1} m, reversed to {backed_to:.1} m)"
    );
    assert_eq!(session.timing().runs, 0, "no lap may be counted");
    assert_eq!(session.timing().best_ticks, 0);
}

#[test]
fn switching_traction_control_off_lets_the_wheels_spin() {
    let config = qts_sim::session::SessionConfig {
        assists: qts_vehicle::Assists {
            auto_clutch: true,
            auto_shift: true,
            traction_control: false,
            engine_damage: false,
        },
        ..Default::default()
    };
    let mut session =
        qts_sim::Session::from_data(&data(), "sedan_halden_e4", "test_track", config).unwrap();
    let rate = f64::from(session.tick_rate_hz());
    let floored = InputFrame::from_analog(0.0, 1.0, 0.0, 0.0, 0.0);
    let limit = 2.0 * session.car().params().traction_control.slip_threshold;
    let mut spinning = 0.0;
    for _ in 0..(3.0 * rate) as u64 {
        session.step(&floored);
        if worst_drive_slip(&session) > limit {
            spinning += 1.0 / rate;
        }
    }
    assert!(
        spinning > 0.5,
        "only {spinning:.2} s of wheelspin with traction control off"
    );
}
