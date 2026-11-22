// SPDX-License-Identifier: Apache-2.0
//! Per-lap telemetry files and the lap comparison (D-081).

mod common;

use common::*;
use qts_core::snapshot::state_hash;
use qts_core::step::Simulation;
use qts_ffb::FfbSample;
use qts_sim::drivers::{Driver, PathFollower, PathFollowerConfig};
use qts_sim::lap_telemetry::{compare, Lap, LapRecorder, LAP_SAMPLE_RATE_HZ};

#[test]
fn each_lap_is_written_with_its_time_and_recording_changes_nothing() {
    let dir = std::env::temp_dir().join(format!("qts_lap_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut a = session("hatch_tern_gt", "ridgeline_circuit");
    let mut b = session("hatch_tern_gt", "ridgeline_circuit");
    let path = a.track().path("circuit").unwrap().clone();
    let config = PathFollowerConfig {
        closed: true,
        ..PathFollowerConfig::default()
    };
    let mut driver = PathFollower::new(&path, config);
    let mut recorder = LapRecorder::new(&dir, &a).unwrap();
    let mut lap_ticks = Vec::new();
    let mut runs = 0;
    for n in 0..330_000_u64 {
        let frame = driver.frame(&a);
        a.step(&frame);
        b.step(&frame);
        // Clipping on one tick in four, to check the per-lap share.
        let ffb = FfbSample {
            clipped: if n % 4 == 0 { 0.5 } else { 0.0 },
            ..FfbSample::default()
        };
        recorder.observe(&a, &frame, &ffb);
        if a.timing().runs > runs {
            runs = a.timing().runs;
            lap_ticks.push(a.timing().last_ticks);
        }
        if runs >= 2 {
            break;
        }
    }
    // The recorder only reads: the recorded and unrecorded sessions agree.
    assert_eq!(state_hash(&a), state_hash(&b));
    assert_eq!(lap_ticks.len(), 2, "two laps driven");
    let written: Vec<_> = recorder.wait().into_iter().map(|r| r.unwrap()).collect();
    assert_eq!(written.len(), 2);
    for (file, ticks) in written.iter().zip(&lap_ticks) {
        let lap = Lap::read(file).unwrap();
        let seconds = *ticks as f64 / 1000.0;
        assert_eq!(lap.file.meta("lap_counted"), Some("true"));
        let stated: f64 = lap.file.meta("lap_time_s").unwrap().parse().unwrap();
        assert!((stated - seconds).abs() < 1e-3);
        let clipped: f64 = lap
            .file
            .meta("ffb_clipped_percent")
            .unwrap()
            .parse()
            .unwrap();
        assert!((clipped - 25.0).abs() < 0.05, "clipped {clipped}%");
        // One row every 10 ms of the lap.
        let rows = lap.file.rows.len() as f64;
        assert!(
            (rows - seconds * LAP_SAMPLE_RATE_HZ).abs() <= 2.0,
            "{rows} rows for {seconds} s"
        );
        // Every asked-for channel is there.
        for col in [
            "speed_m_s",
            "throttle",
            "brake",
            "clutch",
            "steer_input",
            "engine_rpm",
            "gear",
            "fl_slip_ratio",
            "fr_slip_angle_rad",
            "rl_load_n",
            "rr_temp_surface_c",
            "fl_brake_temp_c",
            "fr_tire_wear",
            "steering_torque_nm",
        ] {
            assert!(lap.file.column_index(col).is_some(), "missing {col}");
        }
        // Distance from integrated speed is the lap's length to within a
        // few metres (2.86 km here).
        assert!(
            (lap.length() - path.length()).abs() < 0.02 * path.length(),
            "{} m",
            lap.length()
        );
    }
    // A lap against itself: no difference anywhere.
    let lap = Lap::read(&written[1]).unwrap();
    let c = compare(&lap, &lap, 5.0).unwrap();
    assert!(c.delta.iter().all(|d| d.abs() < 1e-9));
    // Against the other lap: the delta at the end is the difference in
    // lap time, to within the stretch the shorter lap leaves out.
    let first = Lap::read(&written[0]).unwrap();
    let c = compare(&first, &lap, 5.0).unwrap();
    let diff = (lap_ticks[1] as f64 - lap_ticks[0] as f64) / 1000.0;
    assert!(
        (c.delta.last().unwrap() - diff).abs() < 0.1,
        "{} vs {diff}",
        c.delta.last().unwrap()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_clip_meter_reports_this_lap_and_the_last() {
    use qts_sim::lap_telemetry::LapClipMeter;
    let mut m = LapClipMeter::default();
    // Not timing: nothing counted.
    for _ in 0..100 {
        m.observe((0, 0), false, 1.0);
    }
    assert_eq!(m.this_lap_percent(), 0.0);
    // A lap with one tick in ten clipping.
    for n in 0..1000 {
        m.observe((0, 0), true, if n % 10 == 0 { 0.5 } else { 0.0 });
    }
    assert!((m.this_lap_percent() - 10.0).abs() < 1e-9);
    assert_eq!(m.last_lap_percent, None);
    // The line: the lap counts, the next one starts clean.
    m.observe((1, 0), true, 0.0);
    assert!((m.last_lap_percent.unwrap() - 10.0).abs() < 1e-9);
    assert_eq!(m.this_lap_percent(), 0.0);
    // Clipping below the threshold is not counted.
    m.observe((1, 0), true, 0.01);
    assert_eq!(m.this_lap_percent(), 0.0);
}
