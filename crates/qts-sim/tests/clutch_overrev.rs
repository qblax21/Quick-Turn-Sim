// SPDX-License-Identifier: Apache-2.0
//! Clutch, over-rev and the drivetrain FFB cues through a whole session,
//! and their replays.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::replay::{Replay, ReplayRecorder};
use qts_core::step::Simulation;
use qts_sim::host::drivetrain_cues;
use qts_sim::session::{Session, SessionConfig};
use qts_vehicle::Assists;

fn manual_session(damage: bool) -> Session {
    let config = SessionConfig {
        assists: Assists {
            auto_clutch: false,
            auto_shift: false,
            traction_control: true,
            engine_damage: damage,
        },
        ..SessionConfig::default()
    };
    Session::from_data(&data(), "hatch_tern_gt", "test_track", config).unwrap()
}

/// A driver with a clutch pedal: a launch through the bite zone, changes
/// up to third with the clutch, then a missed downshift into first.
/// Calls `each` after every tick.
fn money_shift(s: &mut Session, mut each: impl FnMut(&Session, &InputFrame)) {
    let mut frames = Vec::new();
    let push = |frames: &mut Vec<InputFrame>, n: usize, throttle: f64, clutch: f64, gear: i8| {
        for _ in 0..n {
            frames.push(InputFrame::from_analog(0.0, throttle, 0.0, clutch, 0.0).with_gear(gear));
        }
    };
    push(&mut frames, 400, 0.0, 1.0, 1);
    // Let the clutch out slowly across the pedal's travel.
    for i in 0..1500 {
        let clutch = 1.0 - i as f64 / 1500.0;
        frames.push(InputFrame::from_analog(0.0, 0.35, 0.0, clutch, 0.0).with_gear(1));
    }
    push(&mut frames, 1500, 1.0, 0.0, 1);
    for gear in [2, 3] {
        push(&mut frames, 150, 0.0, 1.0, gear);
        push(&mut frames, 2500, 1.0, 0.0, gear);
    }
    // Clutch down, first instead of third, clutch dropped.
    push(&mut frames, 300, 0.0, 1.0, 1);
    push(&mut frames, 1500, 0.0, 0.0, 1);
    for f in frames {
        s.step(&f);
        each(s, &f);
    }
}

#[test]
fn a_clutch_launch_and_a_missed_downshift_replay_exactly() {
    for damage in [false, true] {
        let mut original = manual_session(damage);
        let mut recorder = ReplayRecorder::start(original.replay_header(25), &original);
        let mut peak_shudder: f64 = 0.0;
        money_shift(&mut original, |s, f| {
            recorder.record(*f, s);
            peak_shudder = peak_shudder.max(drivetrain_cues(&s.car().output().drive).0);
        });
        let d = original.car().output().drive;
        assert!(d.peak_rpm > 7500.0, "peak {} rpm", d.peak_rpm);
        assert_eq!(d.engine_damage > 0.0, damage, "damage {}", d.engine_damage);
        assert!(
            peak_shudder > 0.2,
            "no shudder cue on the launch: {peak_shudder}"
        );

        let replay = Replay::from_bytes(&recorder.finish().to_bytes()).unwrap();
        let mut fresh = manual_session(!damage);
        // The snapshot carries the damage setting, whatever the session
        // was built with.
        let report = replay.verify(&mut fresh).unwrap();
        assert!(
            report.is_exact(),
            "damage {damage}: {:?}",
            report.first_mismatch
        );
        assert_eq!(fresh.config().assists.engine_damage, damage);
        assert_eq!(fresh.car().state, original.car().state);
        assert_eq!(fresh.tick(), original.tick());
    }
}

#[test]
fn forcing_a_gear_without_the_clutch_gives_a_grind_cue() {
    let mut s = manual_session(false);
    for _ in 0..500 {
        s.step(&InputFrame::default());
    }
    let mut grind: f64 = 0.0;
    for _ in 0..300 {
        s.step(&InputFrame::default().with_gear(1));
        grind = grind.max(drivetrain_cues(&s.car().output().drive).1);
    }
    assert!(s.car().output().drive.grinding);
    assert!(grind >= 0.3, "grind cue {grind}");
    assert_eq!(drivetrain_cues(&s.car().output().drive).0, 0.0);
}
