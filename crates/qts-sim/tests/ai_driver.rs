// SPDX-License-Identifier: Apache-2.0
//! The computer driver (D-122): it drives through the same input frame a
//! person makes, with no assists, and laps a circuit at a chosen pace.

mod common;

use common::*;
use qts_sim::ai::{run_laps, AiConfig, AiDriver};
use qts_sim::session::Session;

fn session(car: &str, track: &str) -> Session {
    Session::from_data(&data(), car, track, AiDriver::session_config()).unwrap()
}

#[test]
fn it_drives_with_no_assists() {
    let a = AiDriver::session_config().assists;
    assert!(!a.auto_clutch && !a.auto_shift && !a.traction_control);
}

/// Grip laps of Ridgeline: from a standing start in neutral it finds first
/// gear on the clutch, then laps inside the track limits, and a faster pace
/// gives a faster lap.
#[test]
fn it_laps_ridgeline_and_pace_sets_the_time() {
    let mut times = Vec::new();
    for pace in [0.6, 0.9] {
        let mut s = session("hatch_tern_gt", "ridgeline_circuit");
        let mut ai = AiDriver::new(s.track(), AiConfig::grip(pace)).unwrap();
        // The out lap from the standing start, then one timed lap.
        let run = run_laps(&mut s, &mut ai, 2, 360.0);
        eprintln!("pace {pace}: {:?}, resets {}", run.laps, ai.resets);
        assert_eq!(run.laps.len(), 2, "pace {pace}: laps {:?}", run.laps);
        let (time, counted, _) = run.laps[1];
        assert!(counted, "pace {pace}: the lap broke the track limits");
        assert_eq!(ai.resets, 0);
        times.push(time);
    }
    assert!(
        times[1] < times[0] - 5.0,
        "pace 0.9 lapped in {:.1} s, pace 0.6 in {:.1} s",
        times[1],
        times[0]
    );
}

/// Drift mode on Harrow (D-126): the out lap and the first timed lap are
/// clean judged runs: the lap counts, no corner has a penalty (spin, three
/// tyres off, opposite drift), every corner is judged and the score is
/// above zero, with no resets.
#[test]
fn it_drives_a_clean_judged_harrow_run() {
    let mut s = session("coupe_kestrel_rc", "harrow_drift_park");
    let mut ai = AiDriver::new(s.track(), AiConfig::drift(0.9)).unwrap();
    let run = run_laps(&mut s, &mut ai, 2, 300.0);
    eprintln!("laps {:?}", run.laps);
    for z in &run.zones {
        eprintln!("  {} {:.1} {}", z.0, z.1, z.2);
    }
    assert_eq!(run.laps.len(), 2, "laps {:?}", run.laps);
    assert_eq!(ai.resets, 0);
    let (time, counted, score) = run.laps[1];
    assert!(counted, "the timed lap broke the track limits");
    let score = score.expect("no drift score");
    assert!(score > 15.0, "score {score:.1}");
    assert!(time < 130.0, "lap took {time:.1} s");
    assert_eq!(run.lap_zones.len(), 2);
    for (name, points, why) in &run.lap_zones[1] {
        assert!(*points > 0.0, "{name}: {why}");
    }
}
