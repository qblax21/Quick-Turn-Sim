// SPDX-License-Identifier: Apache-2.0
//! Watching a replay: it re-simulates exactly, shows the car where it was,
//! can be scrubbed both ways and reports a replay that no longer matches.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::replay::ReplayRecorder;
use qts_core::step::Simulation;
use qts_sim::drivers::{Driver, PathFollower, PathFollowerConfig};
use qts_sim::playback::Playback;

/// Records a drive round the cone course, returning the replay's bytes and
/// the car's position after each second.
fn record() -> (Vec<u8>, Vec<[f64; 3]>) {
    let mut s = session("hatch_tern_gt", "autocross_lot");
    let path = s.track().path("course").unwrap().clone();
    let mut driver = PathFollower::new(
        &path,
        PathFollowerConfig {
            max_speed: 14.0,
            ..PathFollowerConfig::default()
        },
    );
    // Let it settle first, so the recording starts mid-session.
    for _ in 0..500 {
        s.step(&InputFrame::default());
    }
    let mut recorder = ReplayRecorder::start(s.replay_header(20), &s);
    let mut positions = Vec::new();
    for tick in 1..=20_000u64 {
        let frame = driver.frame(&s);
        s.step(&frame);
        recorder.record(frame, &s);
        if tick % 1000 == 0 {
            positions.push(s.car().state.pos.to_array());
        }
    }
    (recorder.finish().to_bytes(), positions)
}

#[test]
fn a_replay_plays_back_exactly_and_can_be_scrubbed() {
    let (bytes, positions) = record();
    let mut playback = Playback::load(&data(), &bytes).unwrap();
    assert!((playback.duration_s() - 20.0).abs() < 1e-9);
    // Prepared in slices, as a front end would.
    let mut slices = 0;
    while !playback.is_prepared() {
        let progress = playback.prepare(3000);
        assert!((0.0..=1.0).contains(&progress));
        slices += 1;
    }
    assert!(slices >= 6);
    assert_eq!(
        playback.first_mismatch(),
        None,
        "the replay did not reproduce"
    );

    // Every second lands where the car really was.
    for (i, recorded) in positions.iter().enumerate() {
        let shown = playback.at((i + 1) as f64).pose.pos;
        let gap = (0..3)
            .map(|k| (shown[k] - recorded[k]).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!(gap < 1e-9, "second {}: {gap} m out", i + 1);
    }
    // Scrubbing back gives the same as going forward.
    let forward = playback.at(12.345);
    let _ = playback.at(19.0);
    assert_eq!(playback.at(12.345), forward);
    let moving = playback.at(10.0);
    assert!(moving.speed > 3.0 && moving.run_time_s > 0.0);
    // Before the start it shows the start; past the end, the end.
    assert_eq!(playback.at(-5.0).pose.pos, playback.at(0.0).pose.pos);
    assert_eq!(playback.at(99.0).pose.pos, playback.at(20.0).pose.pos);
    assert_eq!(
        playback.cones_at(15.0).len(),
        playback.session().track().cones().len()
    );
}

#[test]
fn a_replay_that_no_longer_reproduces_is_reported() {
    let (bytes, _) = record();
    let mut replay = qts_core::replay::Replay::from_bytes(&bytes).unwrap();
    // Corrupt one recorded hash: as if the code had changed since.
    replay.hashes[100] ^= 1;
    let mut playback = Playback::load(&data(), &replay.to_bytes()).unwrap();
    while !playback.is_prepared() {
        playback.prepare(5000);
    }
    assert_eq!(playback.first_mismatch(), Some(101 * 20));
}

#[test]
fn a_replay_of_changed_data_is_refused() {
    let (bytes, _) = record();
    let mut replay = qts_core::replay::Replay::from_bytes(&bytes).unwrap();
    replay.header.content_hash ^= 1;
    let error = Playback::load(&data(), &replay.to_bytes()).err().unwrap();
    assert!(error.contains("changed"), "{error}");
}
