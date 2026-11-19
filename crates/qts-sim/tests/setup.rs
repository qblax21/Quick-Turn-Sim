// SPDX-License-Identifier: Apache-2.0
//! Every garage adjustment reaches the physics, and the ones with a clear
//! expected effect have it. Setups travel with replays and ghost laps.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_core::replay::{Replay, ReplayRecorder};
use qts_core::snapshot::state_hash;
use qts_core::step::Simulation;
use qts_sim::session::{Session, SessionAssets, SessionConfig};
use qts_sim::setup::{setup_items, Setup, SetupItem};
use qts_vehicle::Assists;

fn config() -> SessionConfig {
    SessionConfig {
        assists: Assists {
            auto_clutch: true,
            auto_shift: true,
            traction_control: false,
        },
        ..SessionConfig::default()
    }
}

fn build(car: &str, setup: &Setup) -> Session {
    build_with(car, setup, config())
}

fn build_with(car: &str, setup: &Setup, config: SessionConfig) -> Session {
    let assets = SessionAssets::load(&data(), car, "test_track")
        .unwrap()
        .with_setup(setup)
        .unwrap();
    Session::new(assets, config).unwrap()
}

/// Gear held by request alone: no automatic shifting.
fn manual(car: &str, setup: &Setup) -> Session {
    let mut c = config();
    c.assists.auto_shift = false;
    // Traction control keeps the driven wheels turning with the road, so
    // engine speed over wheel speed is the gearing alone.
    c.assists.traction_control = true;
    build_with(car, setup, c)
}

fn items(car: &str) -> Vec<SetupItem> {
    let s = build(car, &Setup::default());
    let assets = SessionAssets::load(&data(), car, "test_track").unwrap();
    let list = setup_items(&assets.car, &assets.tire_front, &assets.tire_rear);
    drop(s);
    list
}

/// The end of the range furthest from the car's own value.
fn far_end(item: &SetupItem) -> f64 {
    if item.max - item.default >= item.default - item.min {
        item.max
    } else {
        item.min
    }
}

fn one(key: &str, value: f64) -> Setup {
    let mut s = Setup::default();
    s.values.insert(key.to_string(), value);
    s
}

/// Straight-line acceleration, a curve on part throttle, then braking.
fn standard_drive(s: &mut Session) {
    for t in 0..6000 {
        let frame = match t {
            0..=2499 => InputFrame::from_analog(0.0, 1.0, 0.0, 0.0, 0.0),
            2500..=4999 => InputFrame::from_analog(0.25, 0.5, 0.0, 0.0, 0.0),
            _ => InputFrame::from_analog(0.1, 0.0, 0.8, 0.0, 0.0),
        };
        s.step(&frame);
    }
}

/// Full throttle up through the gears to `gear`, changing near the
/// limiter, then half a second in `gear` so the clutch is locked.
fn in_gear(s: &mut Session, gear: i8) {
    let redline = match &s.car().params().drivetrain {
        qts_vehicle::params::DrivetrainParams::EngineManual(m) => m.engine.redline_rpm,
        _ => f64::MAX,
    };
    let engaged = |s: &Session, g: i8| {
        let d = s.car().output().drive;
        d.gear == g && !d.shifting
    };
    // Off the throttle while the gear changes, as a driver would.
    let step = |s: &mut Session, g: i8| {
        let throttle = if engaged(s, g) { 1.0 } else { 0.0 };
        s.step(&InputFrame::from_analog(0.0, throttle, 0.0, 0.0, 0.0).with_gear(g));
    };
    for g in 1..gear {
        let mut in_this = 0;
        for _ in 0..8000 {
            step(s, g);
            in_this += u32::from(engaged(s, g));
            if in_this > 300 && s.car().output().drive.engine_rpm > 0.85 * redline {
                break;
            }
        }
    }
    let mut held = 0;
    // Until the clutch has been fully locked for a while.
    for _ in 0..6000 {
        step(s, gear);
        if engaged(s, gear) && s.car().output().drive.clutch_engagement > 0.999 {
            held += 1;
            if held >= 300 {
                break;
            }
        } else {
            held = 0;
        }
    }
}

#[test]
fn every_adjustment_changes_the_physics() {
    for car in CARS {
        let list = items(car);
        assert!(
            list.len() >= 15,
            "{car} offers only {} adjustments",
            list.len()
        );
        let mut baseline = build(car, &Setup::default());
        standard_drive(&mut baseline);
        let base_hash = state_hash(&baseline);
        for item in &list {
            let value = far_end(item);
            let mut s = build(car, &one(&item.key, value));
            if let Some(gear) = item.key.strip_prefix("gear_") {
                let gear: i8 = gear.parse().unwrap();
                let mut base = manual(car, &Setup::default());
                let mut s = manual(car, &one(&item.key, value));
                in_gear(&mut base, gear);
                in_gear(&mut s, gear);
                assert_ne!(
                    state_hash(&s),
                    state_hash(&base),
                    "{car}: {} changed nothing",
                    item.key
                );
                // Engine speed over driven-wheel speed is the overall ratio.
                let ratio_of = |s: &Session| {
                    let out = s.car().output();
                    out.drive.engine_rpm * std::f64::consts::PI
                        / 30.0
                        / (0.5 * (out.wheels[0].omega + out.wheels[1].omega))
                };
                let change = ratio_of(&s) / ratio_of(&base);
                let expected = value / item.default;
                assert!(
                    (change - expected).abs() < 0.02,
                    "{car} {}: ratio changed by {change:.3}, expected {expected:.3}",
                    item.key
                );
                continue;
            }
            standard_drive(&mut s);
            assert_ne!(
                state_hash(&s),
                base_hash,
                "{car}: {} at {value} changed nothing",
                item.key
            );
        }
    }
}

#[test]
fn pressure_camber_and_toe_show_up_on_the_tires_at_rest() {
    let car = "hatch_tern_gt";
    let at_rest = |setup: &Setup| {
        let mut s = build(car, setup);
        for _ in 0..500 {
            s.step(&InputFrame::default());
        }
        s.car().output().wheels
    };
    let base = at_rest(&Setup::default());
    let list = items(car);
    let item = |key: &str| list.iter().find(|i| i.key == key).unwrap().clone();

    let p = item("rear.pressure_kpa");
    let w = at_rest(&one(&p.key, p.default + 40.0));
    assert!((w[2].pressure_pa - base[2].pressure_pa - 40_000.0).abs() < 2_000.0);
    assert!(
        (w[0].pressure_pa - base[0].pressure_pa).abs() < 1.0,
        "front unchanged"
    );

    let c = item("front.camber_deg");
    let w = at_rest(&one(&c.key, c.default - 1.5));
    let moved = (w[0].camber - base[0].camber).abs().to_degrees();
    assert!(
        (moved - 1.5).abs() < 0.2,
        "front camber moved {moved:.2} deg"
    );

    let t = item("front.toe_deg");
    let w = at_rest(&one(&t.key, t.default + 0.3));
    let moved = (w[0].steer_angle - base[0].steer_angle).abs().to_degrees();
    assert!((moved - 0.3).abs() < 0.05, "front toe moved {moved:.3} deg");
    // Toe-in turns the two wheels opposite ways.
    let left = w[0].steer_angle - base[0].steer_angle;
    let right = w[1].steer_angle - base[1].steer_angle;
    assert!(left * right < 0.0);
}

#[test]
fn stiffer_bars_roll_less_and_brake_bias_splits_the_braking() {
    let car = "hatch_tern_gt";
    let list = items(car);
    let item = |key: &str| list.iter().find(|i| i.key == key).unwrap().clone();
    let corner_roll = |setup: &Setup| {
        let mut s = build(car, setup);
        for t in 0..6000 {
            let steer = if t < 2500 { 0.0 } else { 0.12 };
            let throttle = if t < 2500 { 0.8 } else { 0.35 };
            s.step(&InputFrame::from_analog(steer, throttle, 0.0, 0.0, 0.0));
        }
        s.car().state.rot.roll().abs()
    };
    let soft = corner_roll(&one("front.arb_n_per_mm", 0.0));
    let stiff = corner_roll(&one("front.arb_n_per_mm", item("front.arb_n_per_mm").max));
    assert!(
        stiff < 0.9 * soft,
        "roll {soft:.4} rad with no bar, {stiff:.4} with the stiffest"
    );

    for bias in [55.0, 72.0] {
        let mut s = build(car, &one("brake_bias_pct", bias));
        for _ in 0..3000 {
            s.step(&InputFrame::from_analog(0.0, 1.0, 0.0, 0.0, 0.0));
        }
        // Light braking, well below locking, so the split is the bias.
        for _ in 0..200 {
            s.step(&InputFrame::from_analog(0.0, 0.0, 0.25, 0.0, 0.0));
        }
        let w = s.car().output().wheels;
        let front = w[0].brake_torque.abs() + w[1].brake_torque.abs();
        let rear = w[2].brake_torque.abs() + w[3].brake_torque.abs();
        let share = 100.0 * front / (front + rear);
        assert!(
            (share - bias).abs() < 1.0,
            "bias {bias}: front share {share:.1}"
        );
    }
}

#[test]
fn values_outside_the_range_or_unknown_keys_are_refused() {
    let assets = || SessionAssets::load(&data(), "hatch_tern_gt", "test_track").unwrap();
    assert!(assets().with_setup(&one("front.camber_deg", -6.0)).is_err());
    assert!(assets()
        .with_setup(&one("rear.pressure_kpa", 50.0))
        .is_err());
    assert!(assets().with_setup(&one("wing_angle", 3.0)).is_err());
    // The open-differential car has no differential adjustment.
    assert!(assets().with_setup(&one("diff.preload_nm", 50.0)).is_err());
    let lsd = SessionAssets::load(&data(), "hatch_tern_gt_lsd", "test_track").unwrap();
    assert!(lsd.with_setup(&one("diff.preload_nm", 50.0)).is_ok());
    // Gears must stay in order.
    let list = items("hatch_tern_gt");
    let g2 = list.iter().find(|i| i.key == "gear_2").unwrap();
    let g3 = list.iter().find(|i| i.key == "gear_3").unwrap();
    let mut crossed = Setup::default();
    crossed.values.insert("gear_2".into(), g2.min);
    crossed.values.insert("gear_3".into(), g3.max);
    if g2.min <= g3.max {
        assert!(assets().with_setup(&crossed).is_err());
    }
}

#[test]
fn a_replay_carries_its_setup_and_reproduces() {
    let mut setup = Setup {
        name: "test".into(),
        ..Setup::default()
    };
    setup.values.insert("front.camber_deg".into(), -2.4);
    setup.values.insert("rear.arb_n_per_mm".into(), 30.0);
    setup.values.insert("brake_bias_pct".into(), 60.0);
    let mut s = build("hatch_tern_gt", &setup);
    let mut recorder = ReplayRecorder::start(s.replay_header(50), &s);
    for t in 0..3000 {
        let frame = InputFrame::from_analog(if t > 1500 { 0.2 } else { 0.0 }, 0.8, 0.0, 0.0, 0.0);
        s.step(&frame);
        recorder.record(frame, &s);
    }
    let replay = Replay::from_bytes(&recorder.finish().to_bytes()).unwrap();
    assert_eq!(Setup::from_text(&replay.header.setup).unwrap(), setup);
    let mut again = Session::for_replay(&data(), &replay.header).unwrap();
    assert_eq!(again.setup(), &setup);
    let report = replay.verify(&mut again).unwrap();
    assert!(report.is_exact(), "{:?}", report.first_mismatch);
    // Without the setup the same inputs drive differently.
    let mut header = replay.header.clone();
    header.setup = String::new();
    let mut plain = Session::for_replay(&data(), &header).unwrap();
    assert!(!replay.verify(&mut plain).unwrap().is_exact());
}
