// SPDX-License-Identifier: Apache-2.0
//! The real-time host, run for a short stretch of real time without a
//! steering device. Timing assertions are loose: the point is that the
//! thread steps, publishes and shuts down, not how accurately the test
//! machine schedules it.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::replay::Replay;
use qts_core::step::Simulation;
use qts_sim::host::{HostCommand, HostOptions, RealtimeHost};
use qts_telemetry::read_telemetry;
use std::time::Duration;

fn options() -> HostOptions {
    HostOptions {
        ffb: data().load_ffb("generic_wheel").unwrap().value,
        max_catch_up_ticks: 100,
        interpolate_inputs: true,
        ghost_path: None,
    }
}

#[test]
fn host_steps_publishes_records_and_stops() {
    let dir = std::env::temp_dir().join(format!("sim-host-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let telemetry_path = dir.join("run.csv");
    let replay_path = dir.join("run.rpl");

    let host = RealtimeHost::start(session("sedan_halden_e4", "test_track"), options()).unwrap();
    let (start_pose, _) = host.render();
    host.send(HostCommand::StartReplay { hash_interval: 20 });
    host.send(HostCommand::StartTelemetry {
        path: telemetry_path.clone(),
        sample_rate_hz: 100.0,
    });
    for _ in 0..60 {
        host.set_input(InputFrame::from_analog(0.05, 1.0, 0.0, 0.0, 0.0));
        std::thread::sleep(Duration::from_millis(10));
    }
    let (pose, hud) = host.render();
    let status = host.status();
    assert!(hud.tick > 100, "only {} ticks in 0.6 s", hud.tick);
    assert!(hud.speed > 1.0, "car should be moving: {} m/s", hud.speed);
    assert!(pose.pos[0] > start_pose.pos[0] + 0.2);
    assert!(pose.rot.iter().map(|r| r * r).sum::<f64>().sqrt() > 0.999);
    assert!(status.device.is_none());
    assert!(status.last_error.is_none(), "{:?}", status.last_error);
    assert!(status.telemetry_rows.unwrap_or(0) > 5);
    assert!(status.replay_frames.unwrap_or(0) > 100);
    assert!(status.ffb.output.abs() <= 1.0);

    // Pausing stops the clock for the simulation.
    host.send(HostCommand::SetPaused(true));
    std::thread::sleep(Duration::from_millis(60));
    let paused_tick = host.render().1.tick;
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(host.render().1.tick, paused_tick);
    assert!(host.status().paused);
    host.send(HostCommand::SetPaused(false));

    host.send(HostCommand::StopTelemetry);
    host.send(HostCommand::StopReplay {
        path: replay_path.clone(),
    });
    std::thread::sleep(Duration::from_millis(80));
    let final_session = host.stop().expect("thread returns the session");
    assert!(final_session.tick() >= paused_tick);

    // What the host recorded is a valid telemetry file and an exact replay.
    let file = read_telemetry(std::io::BufReader::new(
        std::fs::File::open(&telemetry_path).unwrap(),
    ))
    .unwrap();
    assert!(file.rows.len() > 5);
    assert_eq!(file.meta("track"), Some("test_track"));
    let replay = Replay::from_bytes(&std::fs::read(&replay_path).unwrap()).unwrap();
    assert!(replay.frames.len() > 100);
    let mut fresh = session("sedan_halden_e4", "test_track");
    let report = replay.verify(&mut fresh).unwrap();
    assert!(
        report.is_exact(),
        "host replay diverged: {:?}",
        report.first_mismatch
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opening_a_missing_device_reports_an_error_and_keeps_running() {
    let host = RealtimeHost::start(session("hatch_tern_gt", "autocross_lot"), options()).unwrap();
    host.send(HostCommand::OpenDevice {
        id: "00000000000000000000000000000001".into(),
        window: None,
    });
    // Device enumeration can take a few hundred milliseconds, during which
    // the sim thread is busy with the command.
    let mut status = host.status();
    for _ in 0..100 {
        if status.last_error.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
        status = host.status();
    }
    assert!(status.device.is_none());
    assert!(status.last_error.is_some());
    let before = host.render().1.tick;
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        host.render().1.tick > before,
        "the simulation must keep stepping"
    );
}
