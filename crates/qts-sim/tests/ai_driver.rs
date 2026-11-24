// SPDX-License-Identifier: Apache-2.0
//! The computer driver (D-122): it drives through the same input frame a
//! person makes, with no assists, and laps a circuit at a chosen pace.

mod common;

use common::*;
use qts_core::step::Simulation;
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

/// Drift mode on Harrow: it starts slides in the judged zones. It does not
/// yet hold them well enough for a complete judged run (D-122 says so), so
/// this checks only that it slides, at angle, in the zones.
#[test]
fn it_slides_through_the_first_harrow_zones() {
    let mut s = session("coupe_kestrel_rc", "harrow_drift_park");
    let mut ai = AiDriver::new(s.track(), AiConfig::drift(0.9)).unwrap();
    let first_zone_end = s.track().def().drift.as_ref().unwrap().zones[1].to_m;
    let mut peak: f64 = 0.0;
    for _ in 0..40_000 {
        use qts_sim::drivers::Driver;
        let f = ai.frame(&s);
        s.step(&f);
        let v = s.car().output().vel_body;
        peak = peak.max(v[1].atan2(v[0].abs().max(0.5)).to_degrees().abs());
        if ai.progress_m() > first_zone_end {
            break;
        }
    }
    eprintln!("largest slide through T1 and T2: {peak:.1} deg");
    assert!(peak > 20.0, "no slide: {peak:.1} deg");
    assert!(
        ai.progress_m() > first_zone_end,
        "stopped at {:.0} m",
        ai.progress_m()
    );
}
