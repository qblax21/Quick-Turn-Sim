// SPDX-License-Identifier: Apache-2.0
//! Determinism, replay and snapshot tests on the real data files.
//!
//! These check the guarantee the architecture document states: the same
//! build, given the same snapshot and inputs, produces the same hash
//! sequence. They say nothing about other platforms or builds.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::replay::{Replay, ReplayRecorder};
use qts_core::snapshot::{restore_bytes, snapshot_bytes, state_hash};
use qts_core::step::Simulation;
use qts_sim::drivers::{Driver, Erratic, PathFollower, PathFollowerConfig};

#[test]
fn same_inputs_give_the_same_hash_sequence() {
    for car in CARS {
        for track in TRACKS {
            let mut a = session(car, track);
            let mut b = session(car, track);
            let mut driver = Erratic::new(11);
            for tick in 0..6000 {
                // One driver feeds both sessions, so the inputs are
                // identical by construction.
                let frame = driver.frame(&a);
                a.step(&frame);
                b.step(&frame);
                assert_eq!(
                    state_hash(&a),
                    state_hash(&b),
                    "{car} on {track} diverged at tick {tick}"
                );
            }
        }
    }
}

#[test]
fn different_inputs_give_different_hashes() {
    let mut a = session("hatch_tern_gt", "test_track");
    let mut b = session("hatch_tern_gt", "test_track");
    assert_eq!(state_hash(&a), state_hash(&b));
    for _ in 0..500 {
        a.step(&InputFrame::from_analog(0.0, 0.50, 0.0, 0.0, 0.0));
        b.step(&InputFrame::from_analog(0.0, 0.51, 0.0, 0.0, 0.0));
    }
    assert_ne!(state_hash(&a), state_hash(&b));
}

#[test]
fn recorded_replay_reproduces_the_run_exactly() {
    for (car, track) in [
        ("hatch_tern_gt_lsd", "autocross_lot"),
        ("sedan_halden_e4", "gravel_stage"),
    ] {
        let mut original = session(car, track);
        let mut driver = Erratic::new(5);
        // Run for a while before recording so the replay starts mid-session.
        for _ in 0..1500 {
            let frame = driver.frame(&original);
            original.step(&frame);
        }
        let mut recorder = ReplayRecorder::start(original.replay_header(50), &original);
        for _ in 0..20_000 {
            let frame = driver.frame(&original);
            original.step(&frame);
            recorder.record(frame, &original);
        }
        let replay = recorder.finish();
        assert_eq!(replay.header.start_tick, 1500);
        assert_eq!(replay.hashes.len(), 400);

        // Through the file format and into a fresh session.
        let decoded = Replay::from_bytes(&replay.to_bytes()).unwrap();
        assert_eq!(decoded, replay);
        let mut fresh = session(car, track);
        assert_eq!(fresh.content_hash(), decoded.header.content_hash);
        let report = decoded.verify(&mut fresh).unwrap();
        assert!(
            report.is_exact(),
            "{car} on {track}: {:?}",
            report.first_mismatch
        );
        assert_eq!(report.checkpoints, 400);
        assert_eq!(report.final_hash, state_hash(&original));
        assert_eq!(fresh.tick(), original.tick());
        assert_eq!(fresh.car().state, original.car().state);
        assert_eq!(fresh.timing(), original.timing());
    }
}

#[test]
fn a_tampered_replay_is_caught() {
    let mut original = session("hatch_tern_gt", "test_track");
    let mut driver = Erratic::new(9);
    let mut recorder = ReplayRecorder::start(original.replay_header(100), &original);
    for _ in 0..3000 {
        let frame = driver.frame(&original);
        original.step(&frame);
        recorder.record(frame, &original);
    }
    let mut replay = recorder.finish();
    replay.frames[1234].steer = replay.frames[1234].steer.wrapping_add(4000);
    let mut fresh = session("hatch_tern_gt", "test_track");
    let report = replay.verify(&mut fresh).unwrap();
    assert_eq!(report.first_mismatch.map(|m| m.0), Some(1300));

    // A replay made at another tick rate is refused outright.
    replay.header.tick_rate_hz = 500;
    assert!(replay.verify(&mut fresh).is_err());
}

#[test]
fn snapshot_and_restore_continue_identically() {
    for (car, track) in [
        ("hatch_tern_gt", "gravel_stage"),
        ("sedan_halden_e4", "autocross_lot"),
    ] {
        let mut a = session(car, track);
        let path = a.track().paths()[0].clone();
        let mut driver = PathFollower::new(&path, PathFollowerConfig::default());
        let mut frames = Vec::new();
        for _ in 0..9000 {
            let frame = driver.frame(&a);
            a.step(&frame);
            frames.push(frame);
        }
        let bytes = snapshot_bytes(&a);
        let hash_at_snapshot = state_hash(&a);
        let mut tail = Vec::new();
        for _ in 0..6000 {
            let frame = driver.frame(&a);
            a.step(&frame);
            tail.push((frame, state_hash(&a)));
        }

        // Restore into a session that has done something else entirely.
        let mut b = session(car, track);
        for _ in 0..777 {
            b.step(&InputFrame::from_analog(0.3, 1.0, 0.0, 0.0, 0.0));
        }
        restore_bytes(&mut b, &bytes).unwrap();
        assert_eq!(state_hash(&b), hash_at_snapshot);
        assert_eq!(b.tick(), 9000);
        for (i, (frame, expected)) in tail.iter().enumerate() {
            b.step(frame);
            assert_eq!(
                state_hash(&b),
                *expected,
                "{car} on {track}: tick {i} after restore"
            );
        }

        // A snapshot is rejected by a session with a different cone count.
        let mut other = session(car, "test_track");
        if other.track().cones().len() != a.track().cones().len() {
            assert!(restore_bytes(&mut other, &bytes).is_err());
        }
    }
}

#[test]
fn snapshot_carries_the_assist_settings() {
    let mut with_assists = session("hatch_tern_gt", "test_track");
    with_assists.step(&InputFrame::default());
    let bytes = snapshot_bytes(&with_assists);
    // The default session has automatic gear selection off.
    let mut plain = qts_sim::Session::from_data(
        &data(),
        "hatch_tern_gt",
        "test_track",
        qts_sim::SessionConfig::default(),
    )
    .unwrap();
    assert!(!plain.config().assists.auto_shift);
    restore_bytes(&mut plain, &bytes).unwrap();
    assert!(plain.config().assists.auto_shift);
}
