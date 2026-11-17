// SPDX-License-Identifier: Apache-2.0
//! Solid trees and barriers: the car cannot pass through them, and the
//! tracks place them where their files say.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::step::Simulation;

#[test]
fn trees_and_barriers_stand_where_the_track_files_say() {
    let circuit = session("sedan_halden_e4", "ridgeline_circuit");
    let track = circuit.track();
    assert!(
        track.trees().len() > 300,
        "only {} trees",
        track.trees().len()
    );
    let path = &track.paths()[0];
    for tree in track.trees() {
        // Sample spacing along the path lets a tree sit slightly inside
        // the clear strip between two samples.
        assert!(
            path.project(tree.pos[0], tree.pos[1], 0.0, path.length(), 21.5)
                .is_none(),
            "a tree stands within 21.5 m of the road at {:?}",
            tree.pos
        );
        assert!(tree.trunk_radius > 0.1 && tree.trunk_radius < 0.5);
    }
    assert!(!track.walls().is_empty());
    for wall in track.walls() {
        for end in [wall.a, wall.b] {
            let (_, offset) = path
                .project(end[0], end[1], 0.0, 450.0, 12.0)
                .expect("barriers run beside the start straight");
            assert!((offset.abs() - 11.0).abs() < 0.05, "offset {offset}");
        }
    }
    assert!(!session("hatch_tern_gt", "gravel_stage")
        .track()
        .trees()
        .is_empty());
    assert!(session("hatch_tern_gt", "autocross_lot")
        .track()
        .trees()
        .is_empty());
}

#[test]
fn a_car_driven_at_a_tree_stops_against_it() {
    let mut session = session("sedan_halden_e4", "ridgeline_circuit");
    // A tree with a clear 30 m run up to it from the west.
    let trees = session.track().trees().to_vec();
    let tree = trees
        .iter()
        .find(|t| {
            trees.iter().all(|o| {
                o.pos == t.pos
                    || !(o.pos[0] > t.pos[0] - 35.0
                        && o.pos[0] < t.pos[0] + 1.0
                        && (o.pos[1] - t.pos[1]).abs() < 3.0)
            })
        })
        .copied()
        .expect("some tree has a clear approach");
    let (x, y) = (tree.pos[0] - 30.0, tree.pos[1]);
    let height = session.track().ground_height(x, y);
    session.car_mut().place(x, y, 0.0, height);
    let floored = InputFrame::from_analog(0.0, 1.0, 0.0, 0.0, 0.0);
    let mut top_speed: f64 = 0.0;
    let mut farthest = f64::MIN;
    for _ in 0..6000 {
        session.step(&floored);
        let s = &session.car().state;
        assert!(s.pos.is_finite() && s.vel.is_finite() && s.ang_vel.is_finite());
        top_speed = top_speed.max(s.vel.x);
        farthest = farthest.max(s.pos.x);
    }
    assert!(
        top_speed > 8.0,
        "the car should get going ({top_speed:.1} m/s)"
    );
    assert!(
        farthest < tree.pos[0],
        "the car passed the tree: reached x {farthest:.2}, tree at {:.2}",
        tree.pos[0]
    );
    let speed = session.car().state.vel.length();
    assert!(
        speed < 3.0,
        "still moving at {speed:.1} m/s after the impact"
    );
}

#[test]
fn a_barrier_keeps_the_car_on_its_side() {
    let mut session = session("hatch_tern_gt", "ridgeline_circuit");
    // Steer steadily off the start straight until the barrier is reached.
    let turning = InputFrame::from_analog(0.25, 0.5, 0.0, 0.0, 0.0);
    let mut widest: f64 = 0.0;
    for _ in 0..12_000 {
        session.step(&turning);
        let s = &session.car().state;
        assert!(s.pos.is_finite() && s.vel.is_finite() && s.ang_vel.is_finite());
        if s.pos.x > 5.0 && s.pos.x < 440.0 {
            widest = widest.max(s.pos.y.abs());
        }
    }
    assert!(
        widest > 8.0,
        "the car never reached the barrier ({widest:.1} m)"
    );
    assert!(
        widest < 11.0,
        "the centre of the car got {widest:.2} m from the road centre, past the barrier at 11 m"
    );
}
