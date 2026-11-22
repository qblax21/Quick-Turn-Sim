// SPDX-License-Identifier: Apache-2.0
//! The force-feedback log records exactly one lap, from the start line to
//! the finish, and reads back into a report.

mod common;

use common::*;
use qts_core::step::Simulation;
use qts_ffb::FfbPipeline;
use qts_sim::drivers::{Driver, PathFollower, PathFollowerConfig};
use qts_sim::ffb_log::{report, summarize, FfbLogData, FfbLogStatus, FfbLogger};
use qts_sim::host::ffb_input;

#[test]
fn a_log_covers_one_lap_and_reads_back() {
    let mut s = session("hatch_tern_gt", "autocross_lot");
    let config = data().load_ffb("fanatec_csl_elite").unwrap().value;
    let mut pipeline = FfbPipeline::new(config, s.tick_rate_hz()).unwrap();
    let dir = std::env::temp_dir().join(format!("qts_ffb_log_{}", std::process::id()));
    let path = dir.join("lap.csv");
    let mut log = FfbLogger::arm(path.clone(), &s);
    assert_eq!(log.status(), FfbLogStatus::Waiting);

    let course = s.track().path("course").unwrap().clone();
    let mut driver = PathFollower::new(
        &course,
        PathFollowerConfig {
            max_speed: 20.0,
            ..PathFollowerConfig::default()
        },
    );
    let mut started_at = None;
    for _ in 0..120_000 {
        let frame = driver.frame(&s);
        s.step(&frame);
        let input = ffb_input(&s, None);
        let sample = pipeline.step(&input);
        log.observe(&s, &frame, &input, &sample, pipeline.config());
        if started_at.is_none() && matches!(log.status(), FfbLogStatus::Recording(_)) {
            started_at = Some(s.tick());
        }
        if s.timing().runs > 0 {
            break;
        }
    }
    assert_eq!(s.timing().runs, 1, "the run did not finish");
    let status = log.wait();
    let FfbLogStatus::Saved {
        path: saved,
        lap_counted,
    } = status
    else {
        panic!("not saved: {status:?}");
    };
    assert!(lap_counted);
    assert_eq!(saved, path);

    let text = std::fs::read_to_string(&path).unwrap();
    let parsed = FfbLogData::parse(&text).unwrap();
    assert_eq!(parsed.meta("car"), Some("hatch_tern_gt"));
    assert_eq!(parsed.meta("ffb_preset"), Some("fanatec_csl_elite"));
    assert_eq!(parsed.meta("lap_counted"), Some("true"));
    // One row per tick from the start line up to the tick before the finish.
    assert_eq!(parsed.rows.len() as u64, s.timing().last_ticks);
    let run_time = parsed.column("run_time_s").unwrap();
    assert_eq!(run_time[0], 0.0);
    let lap: f64 = parsed.meta("lap_time_s").unwrap().parse().unwrap();
    assert!((run_time[run_time.len() - 1] - lap).abs() < 0.0015);
    // The logged column torque is the physics torque, unchanged.
    let column = parsed.column("column_torque_nm").unwrap();
    let raw = parsed.column("raw_nm").unwrap();
    assert!(column.iter().zip(&raw).all(|(c, r)| (c - r).abs() < 0.002));
    let summary = summarize(&parsed).unwrap();
    assert!(summary.peak_column_nm > 1.0);
    let r = report(&text).unwrap();
    assert!(r.contains("autocross_lot"));
    assert!(r.contains("puts it at the knee of this"), "{r}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_reset_mid_lap_starts_the_log_again() {
    use qts_core::input::{buttons, InputFrame};
    let mut s = session("hatch_tern_gt", "autocross_lot");
    let config = data().load_ffb("generic_wheel").unwrap().value;
    let mut pipeline = FfbPipeline::new(config, s.tick_rate_hz()).unwrap();
    let path = std::env::temp_dir().join(format!("qts_ffb_reset_{}.csv", std::process::id()));
    let mut log = FfbLogger::arm(path.clone(), &s);
    let course = s.track().path("course").unwrap().clone();
    let mut driver = PathFollower::new(&course, PathFollowerConfig::default());
    for _ in 0..30_000 {
        let frame = driver.frame(&s);
        s.step(&frame);
        let input = ffb_input(&s, None);
        let sample = pipeline.step(&input);
        log.observe(&s, &frame, &input, &sample, pipeline.config());
        if matches!(log.status(), FfbLogStatus::Recording(t) if t > 2.0) {
            break;
        }
    }
    assert!(matches!(log.status(), FfbLogStatus::Recording(_)));
    let frame = InputFrame::default().with_buttons(buttons::RESET);
    s.step(&frame);
    let input = ffb_input(&s, None);
    let sample = pipeline.step(&input);
    log.observe(&s, &frame, &input, &sample, pipeline.config());
    assert_eq!(log.status(), FfbLogStatus::Waiting);
    // Stopping while waiting writes nothing.
    log.stop(&s, pipeline.config());
    assert_eq!(log.wait(), FfbLogStatus::Off);
    assert!(!path.exists());
}
