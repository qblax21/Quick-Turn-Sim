// SPDX-License-Identifier: Apache-2.0
//! The "why am I slow" coach (D-106) on laps driven by the path follower,
//! written and read back as ordinary per-lap telemetry. A reference
//! driver is compared with one that plans corners for less grip (it
//! should be told its minimum speeds are low) and one that plans braking
//! for less deceleration (it should be told it brakes early).

mod common;

use common::*;
use qts_core::step::Simulation;
use qts_ffb::FfbSample;
use qts_sim::coach::{analyse, find_corners, report_text, Cause};
use qts_sim::drivers::{Driver, PathFollower, PathFollowerConfig};
use qts_sim::lap_telemetry::{Lap, LapRecorder};
use std::sync::OnceLock;

/// The second counted lap (a flying lap) of the path follower with the
/// given plan, on Ridgeline.
fn flying_lap(name: &str, lateral_accel: f64, brake_decel: f64) -> Lap {
    let dir = std::env::temp_dir().join(format!("qts_coach_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = session("hatch_tern_gt", "ridgeline_circuit");
    let path = s.track().path("circuit").unwrap().clone();
    let mut driver = PathFollower::new(
        &path,
        PathFollowerConfig {
            closed: true,
            lateral_accel,
            brake_decel,
            ..PathFollowerConfig::default()
        },
    );
    let mut recorder = LapRecorder::new(&dir, &s).unwrap();
    for _ in 0..400_000 {
        let frame = driver.frame(&s);
        s.step(&frame);
        recorder.observe(&s, &frame, &FfbSample::default());
        if s.timing().runs >= 2 {
            break;
        }
    }
    let files: Vec<_> = recorder.wait().into_iter().map(|r| r.unwrap()).collect();
    assert_eq!(files.len(), 2, "{name}: two laps");
    let lap = Lap::read(&files[1]).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    lap
}

struct Laps {
    reference: Lap,
    slow_corners: Lap,
    early_braking: Lap,
}

fn laps() -> &'static Laps {
    static LAPS: OnceLock<Laps> = OnceLock::new();
    LAPS.get_or_init(|| Laps {
        reference: flying_lap("reference", 5.0, 4.0),
        slow_corners: flying_lap("slow_corners", 4.0, 4.0),
        early_braking: flying_lap("early_braking", 5.0, 2.5),
    })
}

#[test]
fn corners_cover_the_lap_in_order() {
    let r = &laps().reference;
    let corners = find_corners(r).unwrap();
    eprintln!(
        "{} corners: {:?}",
        corners.len(),
        corners.iter().map(|c| c.curve).collect::<Vec<_>>()
    );
    assert!(corners.len() >= 5, "only {} corners found", corners.len());
    assert_eq!(corners[0].sector.0, 0.0);
    assert!((corners.last().unwrap().sector.1 - r.length()).abs() < 1e-9);
    for w in corners.windows(2) {
        assert!(
            (w[0].sector.1 - w[1].sector.0).abs() < 1e-9,
            "sectors touch"
        );
        assert!(w[0].curve.1 < w[1].curve.0, "corners in order");
    }
    for c in &corners {
        assert!(c.sector.0 <= c.curve.0 && c.curve.1 <= c.sector.1);
    }
}

#[test]
fn losses_add_up_to_the_lap_time_difference() {
    let l = laps();
    for lap in [&l.slow_corners, &l.early_braking] {
        let rep = analyse(lap, &l.reference).unwrap();
        let sum: f64 = rep.corners.iter().map(|c| c.time_lost_s).sum();
        let diff = rep.lap_s - rep.reference_s;
        assert!(
            (sum - diff).abs() < 1e-6,
            "corners {sum:.4} s, lap {diff:.4} s"
        );
        for c in &rep.corners {
            let phases: f64 = c.phase_lost_s.iter().sum();
            assert!((phases - c.time_lost_s).abs() < 1e-9);
        }
        // Comparing a lap with itself finds nothing.
        let same = analyse(&l.reference, &l.reference).unwrap();
        assert!(same
            .corners
            .iter()
            .all(|c| c.time_lost_s.abs() < 1e-9 && c.cause == Cause::None));
    }
}

#[test]
fn slow_corners_are_blamed_on_minimum_speed() {
    let l = laps();
    let rep = analyse(&l.slow_corners, &l.reference).unwrap();
    eprintln!("slow corners:\n{}", report_text(&rep));
    assert!(
        rep.lap_s > rep.reference_s + 1.0,
        "the slow driver must be slower"
    );
    let losing: Vec<_> = rep
        .corners
        .iter()
        .filter(|c| c.time_lost_s > 0.05)
        .collect();
    let by_speed = losing
        .iter()
        .filter(|c| matches!(c.cause, Cause::LowerMinimumSpeed(_)))
        .count();
    assert!(
        by_speed * 2 > losing.len(),
        "{by_speed} of {} losing corners blamed on minimum speed",
        losing.len()
    );
}

#[test]
fn early_braking_is_blamed_on_braking() {
    let l = laps();
    let rep = analyse(&l.early_braking, &l.reference).unwrap();
    eprintln!("early braking:\n{}", report_text(&rep));
    assert!(
        rep.lap_s > rep.reference_s + 0.5,
        "the early braker must be slower"
    );
    let losing: Vec<_> = rep
        .corners
        .iter()
        .filter(|c| c.time_lost_s > 0.05)
        .collect();
    let by_braking = losing
        .iter()
        .filter(|c| matches!(c.cause, Cause::EarlierBraking(_)))
        .count();
    assert!(
        by_braking * 2 > losing.len(),
        "{by_braking} of {} losing corners blamed on earlier braking",
        losing.len()
    );
}

#[test]
fn the_live_recorder_reports_each_lap_and_keeps_the_best() {
    use qts_sim::coach::CoachRecorder;
    let dir = std::env::temp_dir().join(format!("qts_coach_live_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("best.coach.csv");
    let mut s = session("hatch_tern_gt", "ridgeline_circuit");
    let path = s.track().path("circuit").unwrap().clone();
    let mut driver = PathFollower::new(
        &path,
        PathFollowerConfig {
            closed: true,
            ..PathFollowerConfig::default()
        },
    );
    let mut coach = CoachRecorder::new(&s, Some(file.clone()));
    assert!(!coach.has_reference());
    let slot = coach.slot();
    for _ in 0..400_000 {
        let frame = driver.frame(&s);
        s.step(&frame);
        coach.observe(&s, &frame);
        if s.timing().runs >= 2 {
            break;
        }
    }
    coach.wait();
    let got = slot.lock().unwrap().clone();
    assert!(got.error.is_none(), "{:?}", got.error);
    // The first counted lap became the reference; the second was compared.
    assert_eq!(got.serial, 1);
    let report = got.report.unwrap();
    assert!(report.corners.len() >= 5);
    let sum: f64 = report.corners.iter().map(|c| c.time_lost_s).sum();
    assert!((sum - (report.lap_s - report.reference_s)).abs() < 1e-6);
    // The best lap was saved, and a new session for the same car and track
    // starts with it.
    assert!(file.exists());
    let again = CoachRecorder::new(
        &session("hatch_tern_gt", "ridgeline_circuit"),
        Some(file.clone()),
    );
    assert!(again.has_reference());
    // A different car does not take it.
    let other = CoachRecorder::new(&session("sedan_halden_e4", "ridgeline_circuit"), Some(file));
    assert!(!other.has_reference());
    let _ = std::fs::remove_dir_all(&dir);
}
