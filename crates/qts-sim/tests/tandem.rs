// SPDX-License-Identifier: Apache-2.0
//! Tandem judging (D-137) on Harrow: the computer drives both cars, the
//! lead started ahead as the game's tandem mode starts it, and the judge
//! scores the chase car's lap against it.

mod common;

use common::*;
use qts_core::step::Simulation;
use qts_sim::ai::{AiConfig, AiDriver};
use qts_sim::drift::DriftJudge;
use qts_sim::drivers::Driver;
use qts_sim::host::Opponent;
use qts_sim::session::Session;
use qts_sim::tandem::{TandemJudge, TandemState};

const CAR: &str = "coupe_kestrel_rc";
const TRACK: &str = "harrow_drift_park";

/// One lap of the chase car behind a lead started `ahead_m` along the line.
/// Returns the tandem state after the lap and the judge's mean cost per
/// tick, microseconds.
fn tandem_lap(ahead_m: f64, chase_pace: f64) -> (TandemState, f64) {
    let root = data();
    let mut chase = Session::from_data(&root, CAR, TRACK, AiDriver::session_config()).unwrap();
    let mut chase_ai = AiDriver::new(chase.track(), AiConfig::drift(chase_pace)).unwrap();
    let lead_s = Session::from_data(&root, CAR, TRACK, AiDriver::session_config()).unwrap();
    let lead_ai = AiDriver::new(lead_s.track(), AiConfig::drift(0.85)).unwrap();
    let mut lead = Opponent::new(lead_s, lead_ai, ahead_m);
    let drift = DriftJudge::new(&chase);
    let slot = drift.slot();
    let mut judge = TandemJudge::new(&chase, slot.clone());
    let mut spent = std::time::Duration::ZERO;
    let mut ticks = 0u32;
    while chase.timing().runs + chase.timing().invalid_runs < 1 {
        let f = chase_ai.frame(&chase);
        chase.step(&f);
        let g = lead.driver.frame(&lead.session);
        lead.session.step(&g);
        let t0 = std::time::Instant::now();
        judge.observe(&chase, &lead.session);
        spent += t0.elapsed();
        ticks += 1;
        assert!(ticks < 300_000, "no lap in 300 s");
    }
    let state = slot.lock().unwrap().tandem.clone().unwrap();
    (state, spent.as_secs_f64() * 1e6 / f64::from(ticks))
}

fn show(label: &str, s: &TandemState, cost_us: f64) {
    eprintln!(
        "{label}: tandem lap {:.1} / 100, judge {cost_us:.2} us a tick",
        s.last_lap_total
    );
    for z in s.last_lap.iter().flatten() {
        eprintln!(
            "  {:4} {:5.1}  close {:.2} (gap {:4.1} m)  angle {:.2}  line {:.2}  contacts {}  {}",
            z.name,
            z.total,
            z.proximity,
            z.mean_gap_m,
            z.angle,
            z.line,
            z.contacts,
            if z.lost { "LOST" } else { "" }
        );
    }
    if let Some(c) = &s.coach {
        eprintln!("  coach: {c}");
    }
}

#[test]
fn chasing_the_lead_close_scores_and_falling_back_loses_it() {
    let (close, cost) = tandem_lap(10.0, 0.85);
    show("lead 10 m ahead, same pace", &close, cost);
    let (far, cost_far) = tandem_lap(40.0, 0.85);
    show("lead 40 m ahead", &far, cost_far);

    let zones = close.last_lap.len();
    let judged: Vec<_> = close.last_lap.iter().flatten().collect();
    assert!(judged.len() == zones, "every zone judged");
    // The same driver in the same car close behind: it keeps the lead
    // through most zones and copies its line and angle well.
    let kept = judged.iter().filter(|z| !z.lost).count();
    assert!(kept * 4 >= zones * 3, "kept the lead in {kept} of {zones}");
    let mean = |f: &dyn Fn(&qts_sim::tandem::TandemZone) -> f64| {
        judged.iter().map(|z| f(z)).sum::<f64>() / judged.len() as f64
    };
    assert!(mean(&|z| z.line) > 0.8 && mean(&|z| z.angle) > 0.7);
    assert!(close.last_lap_total > 50.0);
    // 40 m back is past the lost-the-lead gap through most of the lap: the
    // score falls away and the coach says why.
    let lost = far.last_lap.iter().flatten().filter(|z| z.lost).count();
    assert!(lost * 2 >= zones, "lost the lead in {lost} of {zones}");
    assert!(far.coach.as_deref().unwrap_or("").contains("lost the lead"));
    assert!(close.last_lap_total > far.last_lap_total + 30.0);
    // Cheap: well under 1% of the 1000 us tick.
    assert!(cost < 10.0 && cost_far < 10.0);
}
