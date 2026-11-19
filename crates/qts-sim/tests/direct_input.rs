// SPDX-License-Identifier: Apache-2.0
//! Controls read on the physics thread drive the car, replays still
//! reproduce exactly, and the host reports how late input arrives.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::replay::Replay;
use qts_core::step::Simulation;
use qts_ffb::{DeviceInfo, FfbError, InputSource, RawInputState};
use qts_sim::direct_input::{AxisAction, AxisBinding, DirectInputMap, DirectInputs};
use qts_sim::host::{HostCommand, HostOptions, RealtimeHost};
use qts_sim::Session;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A device whose state the test changes while the host reads it.
struct SharedDevice {
    info: DeviceInfo,
    state: Arc<Mutex<RawInputState>>,
}

impl InputSource for SharedDevice {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }
    fn read_state(&mut self) -> Result<RawInputState, FfbError> {
        Ok(*self.state.lock().unwrap())
    }
}

fn pedal(action: AxisAction, axis: usize) -> AxisBinding {
    AxisBinding {
        action,
        device: "pedals".into(),
        axis,
        min: -1.0,
        center: -1.0,
        max: 1.0,
        invert: false,
    }
}

#[test]
fn pedals_read_on_the_physics_thread_drive_the_car_and_replay_exactly() {
    let state = Arc::new(Mutex::new(RawInputState::default()));
    let map = DirectInputMap {
        axes: vec![
            pedal(AxisAction::Throttle, 1),
            AxisBinding {
                action: AxisAction::Steer,
                device: "pedals".into(),
                axis: 0,
                min: -1.0,
                center: 0.0,
                max: 1.0,
                invert: false,
            },
        ],
        buttons: vec![],
    };
    let shared = Arc::clone(&state);
    let (direct, errors) = DirectInputs::open(map, move |id| {
        Ok(Box::new(SharedDevice {
            info: DeviceInfo {
                id: id.into(),
                name: "test pedals".into(),
                has_force_feedback: false,
                backend: "test",
            },
            state: Arc::clone(&shared),
        }) as Box<dyn InputSource>)
    });
    assert!(errors.is_empty());

    let start = session("hatch_tern_gt", "test_track");
    let start_bytes = qts_core::snapshot::state_hash(&start);
    let host = RealtimeHost::start(
        start,
        HostOptions {
            ffb: data().load_ffb("generic_wheel").unwrap().value,
            max_catch_up_ticks: 100,
            interpolate_inputs: true,
            ghost_path: None,
        },
    )
    .unwrap();
    host.send(HostCommand::StartReplay { hash_interval: 10 });
    host.send(HostCommand::SetDirectInputs(Some(Box::new(direct))));
    // The front end sends nothing but an idle frame; the pedal is pressed
    // on the device, and the wheel turned and moved about.
    for i in 0..150 {
        {
            let mut s = state.lock().unwrap();
            s.axes[1] = 0.6;
            s.axes[0] = -0.2 + 0.1 * ((i as f32) * 0.2).sin();
        }
        host.set_input(InputFrame::default());
        std::thread::sleep(Duration::from_millis(10));
    }
    let (_, hud) = host.render();
    assert!(
        hud.speed > 2.0,
        "the device's pedal should drive: {} m/s",
        hud.speed
    );
    assert!(
        (hud.input[1] - 0.8).abs() < 1e-3,
        "throttle used {}",
        hud.input[1]
    );
    assert!(
        hud.input[0] > 0.0,
        "left of centre on the device is left in the sim"
    );
    let status = host.status();
    assert!(status.direct_inputs);
    assert!(status.last_error.is_none(), "{:?}", status.last_error);
    let delay = status.input_delay;
    assert!(
        delay.direct_mean_ms > 0.0 && delay.direct_mean_ms < 5.0,
        "{delay:?}"
    );
    assert!(delay.frame_mean_ms > 0.0, "{delay:?}");

    let path = std::env::temp_dir().join(format!("qts_direct_{}.rpl", std::process::id()));
    host.send(HostCommand::StopReplay { path: path.clone() });
    std::thread::sleep(Duration::from_millis(50));
    let finished = host.stop().unwrap();

    // The replay holds the frames the physics used, so re-simulating it
    // from the start reaches the same state, hash for hash.
    let replay = Replay::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(replay.frames.len() > 1000);
    let mut again: Session = session("hatch_tern_gt", "test_track");
    assert_eq!(qts_core::snapshot::state_hash(&again), start_bytes);
    let report = replay.verify(&mut again).unwrap();
    assert!(report.is_exact(), "{report:?}");
    assert!(finished.tick() >= again.tick());
}
