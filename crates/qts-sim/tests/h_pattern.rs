// SPDX-License-Identifier: Apache-2.0
//! H-pattern shifting through a whole session (D-110): the lever's gate
//! arrives as the input frame's gear request, as both the physics-thread
//! reader (`DirectInputs`) and the front end send it. The mapping itself
//! (gates to a gear, neutral between them) is tested in
//! `direct_input.rs`.

mod common;

use common::*;
use qts_core::input::{InputFrame, GEAR_NO_REQUEST};
use qts_core::step::Simulation;
use qts_sim::session::{Session, SessionConfig};
use qts_vehicle::Assists;

fn session_with(auto_clutch: bool, auto_shift: bool) -> Session {
    let config = SessionConfig {
        assists: Assists {
            auto_clutch,
            auto_shift,
            traction_control: false,
            engine_damage: false,
        },
        ..SessionConfig::default()
    };
    Session::from_data(&data(), "hatch_tern_gt", "test_track", config).unwrap()
}

/// A frame with the lever in `gate`, the clutch pedal at `clutch` (1 is
/// fully down) and the accelerator at `throttle`.
fn lever(gate: i8, clutch: f64, throttle: f64) -> InputFrame {
    let mut f = InputFrame::from_analog(0.0, throttle, 0.0, clutch, 0.0);
    f.gear_request = gate;
    f
}

fn run(s: &mut Session, frame: &InputFrame, ticks: usize) {
    for _ in 0..ticks {
        s.step(frame);
    }
}

fn gear(s: &Session) -> i8 {
    s.car().output().drive.gear
}

#[test]
fn with_a_manual_clutch_a_gear_needs_the_pedal_down() {
    let mut s = session_with(false, false);
    // Engine idling, lever in neutral, clutch up.
    run(&mut s, &lever(0, 0.0, 0.0), 500);
    assert_eq!(gear(&s), 0);
    // Into first with the clutch up: it grinds and stays in neutral.
    run(&mut s, &lever(1, 0.0, 0.0), 200);
    assert_eq!(gear(&s), 0, "a gear went in with the clutch up");
    assert!(s.car().output().drive.grinding, "no grind reported");
    // Clutch down: first goes in.
    run(&mut s, &lever(1, 1.0, 0.0), 400);
    assert_eq!(gear(&s), 1);
    assert!(!s.car().output().drive.grinding);
    // Clutch out gently with some throttle: the car moves off in first.
    for k in 0..3000 {
        let clutch = (1.0 - k as f64 / 1500.0).max(0.0);
        s.step(&lever(1, clutch, 0.35));
    }
    assert_eq!(gear(&s), 1);
    assert!(
        s.car().output().speed > 2.0,
        "{} m/s",
        s.car().output().speed
    );
    // Lever to neutral: out of gear at once, clutch or not.
    run(&mut s, &lever(0, 0.0, 0.0), 5);
    assert_eq!(gear(&s), 0);
}

#[test]
fn reverse_and_every_forward_gate_engage_with_the_clutch_down() {
    let mut s = session_with(false, false);
    run(&mut s, &lever(0, 1.0, 0.0), 500);
    for gate in [-1, 1, 2, 3, 4, 5, 6] {
        // Through neutral, as a real lever goes.
        run(&mut s, &lever(0, 1.0, 0.0), 50);
        run(&mut s, &lever(gate, 1.0, 0.0), 400);
        assert_eq!(gear(&s), gate, "gate {gate}");
    }
}

#[test]
fn with_the_automatic_clutch_the_lever_alone_selects_gears() {
    let mut s = session_with(true, false);
    run(&mut s, &lever(0, 0.0, 0.0), 500);
    run(&mut s, &lever(1, 0.0, 0.0), 400);
    assert_eq!(gear(&s), 1, "first did not go in on the automatic clutch");
    run(&mut s, &lever(1, 0.0, 0.5), 3000);
    assert!(s.car().output().speed > 3.0);
    run(&mut s, &lever(2, 0.0, 0.5), 400);
    assert_eq!(gear(&s), 2);
}

#[test]
fn the_lever_overrides_automatic_gears() {
    // With "Automatic gears" ticked as well, the lever still decides: the
    // box stays where the lever is, even at the limiter.
    let mut s = session_with(true, true);
    run(&mut s, &lever(0, 0.0, 0.0), 500);
    run(&mut s, &lever(1, 0.0, 1.0), 6000);
    assert_eq!(
        gear(&s),
        1,
        "automatic gears moved the box away from the lever"
    );
    // No lever request: automatic gears work as before.
    let mut f = InputFrame::from_analog(0.0, 1.0, 0.0, 0.0, 0.0);
    f.gear_request = GEAR_NO_REQUEST;
    run(&mut s, &f, 4000);
    assert!(
        gear(&s) > 1,
        "automatic gears did not shift without a lever"
    );
}
