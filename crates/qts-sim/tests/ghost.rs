// SPDX-License-Identifier: Apache-2.0
//! The best-lap ghost follows real timed runs.

mod common;

use common::*;
use qts_core::input::{buttons, InputFrame};
use qts_core::step::Simulation;
use qts_sim::drivers::{Driver, PathFollower, PathFollowerConfig};
use qts_sim::ghost::{GhostLap, GhostRecorder, GHOST_SAMPLE_TICKS};
use qts_sim::host::RenderPose;
use qts_sim::Session;

fn pose(s: &Session) -> RenderPose {
    let state = &s.car().state;
    RenderPose {
        tick: s.tick(),
        pos: state.pos.to_array(),
        rot: [state.rot.w, state.rot.x, state.rot.y, state.rot.z],
        ..RenderPose::default()
    }
}

/// Drives one autocross run with the scripted driver. Returns whether the
/// ghost improved and the largest distance between car and ghost while the
/// ghost was showing, how many ticks it showed for, and the last time
/// difference to the best lap reported before the finish (NaN if none).
fn run(s: &mut Session, ghost: &mut GhostRecorder, max_speed: f64) -> (bool, f64, u32, f64) {
    let path = s.track().path("course").unwrap().clone();
    let config = PathFollowerConfig {
        max_speed,
        ..PathFollowerConfig::default()
    };
    let mut driver = PathFollower::new(&path, config);
    let runs_before = s.timing().runs;
    let (mut improved, mut worst, mut shown) = (false, 0.0f64, 0u32);
    let mut delta = f64::NAN;
    for _ in 0..90_000 {
        let frame = driver.frame(s);
        s.step(&frame);
        let now = pose(s);
        improved |= ghost.observe(s, &now);
        if let Some(d) = ghost.delta_s() {
            delta = d;
        }
        if let Some(g) = ghost.pose_now(s, 0.0) {
            let d = (0..3).map(|i| (g.pos[i] - now.pos[i]).powi(2)).sum::<f64>();
            worst = worst.max(d.sqrt());
            shown += 1;
        }
        if s.timing().runs > runs_before {
            break;
        }
    }
    assert_eq!(s.timing().runs, runs_before + 1, "run did not finish");
    (improved, worst, shown, delta)
}

fn reset(s: &mut Session, ghost: &mut GhostRecorder) {
    s.step(&InputFrame::default().with_buttons(buttons::RESET));
    let now = pose(s);
    ghost.observe(s, &now);
    s.step(&InputFrame::default());
    let now = pose(s);
    ghost.observe(s, &now);
}

#[test]
fn the_fastest_run_becomes_the_ghost_and_plays_back_beside_the_car() {
    let mut s = session("hatch_tern_gt", "autocross_lot");
    let mut ghost = GhostRecorder::new(None);

    // First run, driven slowly: there is nothing to show, and it becomes
    // the ghost.
    let (improved, _, shown, delta) = run(&mut s, &mut ghost, 7.0);
    assert!(improved && shown == 0);
    assert!(delta.is_nan(), "no best lap yet, so no time difference");
    let slow = ghost.best().unwrap().clone();
    assert_eq!(slow.lap_ticks, s.timing().last_ticks);
    assert!(slow.fits(&s));
    let expected = slow.lap_ticks / u64::from(GHOST_SAMPLE_TICKS) + 1;
    assert!((slow.poses.len() as u64).abs_diff(expected) <= 1);
    // It starts at the start gate, 2 m along the course.
    let (gate, _) = s.track().path("course").unwrap().at(2.0);
    let first = slow.poses[0].pos;
    assert!((first[0] - gate[0]).abs() < 0.5 && (first[1] - gate[1]).abs() < 0.5);

    // Second run, faster: the ghost shows, falls behind, and is replaced.
    reset(&mut s, &mut ghost);
    let (improved, worst, shown, delta) = run(&mut s, &mut ghost, 30.0);
    assert!(improved && shown > 1000);
    assert!(
        worst > 5.0,
        "the faster car should pull away, gap {worst} m"
    );
    let fast = ghost.best().unwrap().clone();
    assert!(fast.lap_ticks < slow.lap_ticks);
    // Just before the line the time difference is the gap between the laps.
    let gained = (fast.lap_ticks as f64 - slow.lap_ticks as f64) / 1000.0;
    assert!(
        (delta - gained).abs() < 0.5,
        "difference {delta:.2} s at the finish, laps differ by {gained:.2} s"
    );

    // Third run at the first pace: slower than the ghost, so it stays.
    reset(&mut s, &mut ghost);
    let (improved, _, _, delta) = run(&mut s, &mut ghost, 7.0);
    assert!(!improved);
    assert!(
        (delta + gained).abs() < 0.5,
        "difference {delta:.2} s at the finish, expected about {:.2} s",
        -gained
    );
    assert_eq!(ghost.best().unwrap(), &fast);

    // A saved ghost is refused by a session with another car.
    let loaded = GhostLap::from_bytes(&fast.to_bytes()).unwrap();
    assert!(loaded.fits(&s));
    assert!(!loaded.fits(&session("sedan_halden_e4", "autocross_lot")));
}
