// SPDX-License-Identifier: Apache-2.0
//! Only laps that took the course set a time or become the ghost: every
//! checkpoint crossed forward and in order, most of the course covered and
//! a plausible time. Saved ghosts that fail this are ignored on load.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::step::Simulation;
use qts_sim::drivers::{Driver, PathFollower, PathFollowerConfig};
use qts_sim::ghost::{GhostLap, GhostRecorder, GHOST_SAMPLE_TICKS};
use qts_sim::host::{HostOptions, RealtimeHost, RenderPose};
use qts_sim::Session;
use std::time::Duration;

fn pose(s: &Session) -> RenderPose {
    let state = &s.car().state;
    RenderPose {
        tick: s.tick(),
        pos: state.pos.to_array(),
        rot: [state.rot.w, state.rot.x, state.rot.y, state.rot.z],
        ..RenderPose::default()
    }
}

fn drive(s: &mut Session, ghost: &mut GhostRecorder, frame: InputFrame, seconds: f64) {
    let ticks = (seconds * f64::from(s.tick_rate_hz())) as u64;
    for _ in 0..ticks {
        s.step(&frame);
        let now = pose(s);
        ghost.observe(s, &now);
    }
}

/// Teleports the car to `at_m` along the circuit, facing along it, at rest.
fn place_on_circuit(s: &mut Session, at_m: f64) {
    let (p, heading) = s.track().path("circuit").unwrap().at(at_m);
    let height = s.track().ground_height(p[0], p[1]);
    s.car_mut().place(p[0], p[1], heading, height);
}

#[test]
fn a_lap_that_skips_the_course_sets_no_time_and_no_ghost() {
    let mut s = session("sedan_halden_e4", "ridgeline_circuit");
    let mut ghost = GhostRecorder::new(None);
    let gentle = InputFrame::from_analog(0.0, 0.4, 0.0, 0.0, 0.0);

    // Over the line, then a shortcut straight back to just before it. The
    // car crosses the line again forward but has missed every checkpoint.
    drive(&mut s, &mut ghost, gentle, 6.0);
    assert!(s.timing().running, "the run starts at the line");
    place_on_circuit(&mut s, 2830.0);
    drive(&mut s, &mut ghost, gentle, 8.0);
    assert_eq!(s.timing().runs, 0, "a shortcut must not count as a lap");
    assert_eq!(s.timing().best_ticks, 0);
    assert_eq!(s.timing().invalid_runs, 1);
    assert!(
        ghost.best().is_none(),
        "a shortcut must not become the ghost"
    );

    // The same after passing only the first two checkpoints.
    place_on_circuit(&mut s, 2830.0);
    drive(&mut s, &mut ghost, gentle, 8.0);
    for at in [690.0, 1390.0] {
        place_on_circuit(&mut s, at);
        drive(&mut s, &mut ghost, gentle, 3.0);
    }
    place_on_circuit(&mut s, 2830.0);
    drive(&mut s, &mut ghost, gentle, 8.0);
    assert_eq!(s.timing().runs, 0);
    assert!(s.timing().invalid_runs >= 2);
    assert!(ghost.best().is_none());
}

#[test]
fn a_real_lap_counts_and_becomes_the_ghost() {
    let mut s = session("hatch_tern_gt", "ridgeline_circuit");
    let path = s.track().path("circuit").unwrap().clone();
    let mut driver = PathFollower::new(
        &path,
        PathFollowerConfig {
            max_speed: 45.0,
            lateral_accel: 6.0,
            brake_decel: 5.0,
            closed: true,
            ..PathFollowerConfig::default()
        },
    );
    let mut ghost = GhostRecorder::new(None);
    for _ in 0..240_000 {
        let frame = driver.frame(&s);
        s.step(&frame);
        let now = pose(&s);
        ghost.observe(&s, &now);
        if s.timing().runs == 1 {
            break;
        }
    }
    assert_eq!(s.timing().runs, 1, "the lap did not count");
    assert_eq!(s.timing().invalid_runs, 0);
    let lap = ghost.best().expect("the lap became the ghost");
    assert!(lap.is_valid_lap(&s));
    // A lap survives being saved and read back.
    let loaded = GhostLap::from_bytes(&lap.to_bytes()).unwrap();
    assert!(loaded.fits(&s) && loaded.is_valid_lap(&s));
}

/// A lap like the one an earlier build saved: 11.7 s, the car rolling a few
/// metres back and forth over the line.
fn bogus_lap(s: &Session) -> GhostLap {
    let lap_ticks = 11_657;
    let count = lap_ticks / u64::from(GHOST_SAMPLE_TICKS) + 1;
    let poses = (0..count)
        .map(|i| {
            let x = 30.0 + 8.0 * ((i as f64) * 0.02).sin();
            RenderPose {
                tick: i * u64::from(GHOST_SAMPLE_TICKS),
                pos: [x, 0.0, 0.5],
                rot: [1.0, 0.0, 0.0, 0.0],
                ..RenderPose::default()
            }
        })
        .collect();
    GhostLap {
        car_id: s.car().params().id.clone(),
        track_id: s.track().def().id.clone(),
        tick_rate_hz: s.tick_rate_hz(),
        lap_ticks,
        sample_ticks: GHOST_SAMPLE_TICKS,
        poses,
        setup: String::new(),
        drift_score: None,
    }
}

#[test]
fn an_invalid_saved_ghost_is_ignored_on_load() {
    let s = session("sedan_halden_e4", "ridgeline_circuit");
    let bogus = bogus_lap(&s);
    assert!(bogus.fits(&s), "same car, track and rate");
    assert!(!bogus.is_valid_lap(&s), "but not a lap of the track");

    let dir = std::env::temp_dir().join(format!("sim-ghost-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sedan_halden_e4__ridgeline_circuit.ghost");
    std::fs::write(&path, bogus.to_bytes()).unwrap();
    let host = RealtimeHost::start(
        s,
        HostOptions {
            ffb: data().load_ffb("generic_wheel").unwrap().value,
            max_catch_up_ticks: 100,
            interpolate_inputs: true,
            ghost_path: Some(path.clone()),
        },
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let (_, ghost_time) = host.ghost();
    drop(host);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(ghost_time, 0.0, "the invalid ghost should not be shown");
}
