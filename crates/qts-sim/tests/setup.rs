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
            engine_damage: false,
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

/// Overrun and drive torque through the differential in a turn: from a
/// rolling start at 50 km/h in second gear, a lift, part throttle and a
/// lift again with the front wheels at 5 degrees, so the inner and outer
/// wheels want different speeds. A rolling start, because a standing one
/// on a powerful rear-drive car (the Kestrel RC, D-111) is a burnout with
/// both wheels spinning alike.
fn split_launch(s: &mut Session) {
    let (x, y, h) = s.track().spawn();
    s.car_mut().place(x, y, h, 0.0);
    s.car_mut().set_forward_speed(14.0);
    let p = s.car().params().steering.clone();
    let steer = 5_f64.to_radians() * p.ratio / p.max_wheel_angle_rad;
    for t in 0..3000 {
        let throttle = if (1000..2000).contains(&t) { 0.5 } else { 0.0 };
        s.step(&InputFrame::from_analog(steer, throttle, 0.0, 0.0, 0.0).with_gear(2));
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
            if item.key.starts_with("clutch.") {
                // The pedal curve only matters with the pedal in use: a
                // launch with the pedal held part-way.
                let launch = |s: &mut Session| {
                    for t in 0..3000 {
                        let clutch = if t < 500 { 1.0 } else { 0.45 };
                        let frame =
                            InputFrame::from_analog(0.0, 0.3, 0.0, clutch, 0.0).with_gear(1);
                        s.step(&frame);
                    }
                };
                let mut base = manual(car, &Setup::default());
                let mut s = manual(car, &one(&item.key, value));
                launch(&mut base);
                launch(&mut s);
                assert_ne!(
                    state_hash(&s),
                    state_hash(&base),
                    "{car}: {} at {value} changed nothing",
                    item.key
                );
                continue;
            }
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
                // The driven wheels are the fronts or the rears.
                let ratio_of = |s: &Session| {
                    let rear = matches!(
                        &s.car().params().drivetrain,
                        qts_vehicle::params::DrivetrainParams::EngineManual(m)
                            if m.driven_axle == qts_vehicle::params::DrivenAxle::Rear
                    );
                    let (a, b) = if rear { (2, 3) } else { (0, 1) };
                    let out = s.car().output();
                    out.drive.engine_rpm * std::f64::consts::PI
                        / 30.0
                        / (0.5 * (out.wheels[a].omega + out.wheels[b].omega))
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
            if item.key.starts_with("diff.") && item.key != "diff.mode" {
                // Preload and ramps act only with a clutch pack fitted:
                // compare against a clutch pack with the car's settings.
                // Ramps are compared with no preload: on a car whose preload
                // alone holds the differential through the lift (the
                // Kestrel RC, D-111) a ramp that adds nothing would show no
                // change for a reason that is not the ramp's.
                let pack = |extra: Option<(&str, f64)>| {
                    let mut setup = one("diff.mode", 1.0);
                    if item.key != "diff.preload_nm" {
                        setup.values.insert("diff.preload_nm".to_string(), 0.0);
                    }
                    if let Some((k, v)) = extra {
                        setup.values.insert(k.to_string(), v);
                    }
                    let mut s = manual(car, &setup);
                    split_launch(&mut s);
                    state_hash(&s)
                };
                assert_ne!(
                    pack(Some((&item.key, value))),
                    pack(None),
                    "{car}: {} at {value} changed nothing",
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
    // The open-differential car can be fitted with a clutch pack (D-084);
    // the electric sedan has no mechanical differential to set.
    assert!(assets().with_setup(&one("diff.preload_nm", 50.0)).is_ok());
    assert!(assets().with_setup(&one("diff.mode", 3.0)).is_err());
    let sedan = SessionAssets::load(&data(), "sedan_halden_e4", "test_track").unwrap();
    assert!(sedan.with_setup(&one("diff.preload_nm", 50.0)).is_err());
    // Setups written before D-084 as lock percentages still load.
    assert!(assets()
        .with_setup(&one("diff.lock_drive_pct", 30.0))
        .is_ok());
    assert!(assets()
        .with_setup(&one("diff.lock_drive_pct", 95.0))
        .is_err());
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

#[test]
fn a_setup_sheet_reads_back_as_the_same_setup() {
    use qts_sim::setup::setup_sheet;
    let root = data();
    for car_id in CARS {
        let car = root.load_car(car_id).unwrap().value;
        let tf = root.load_tire(&car.tire_front).unwrap().value;
        let tr = root.load_tire(&car.tire_rear).unwrap().value;
        let mut setup = Setup {
            name: "Ridgeline dry".into(),
            ..Setup::default()
        };
        setup.values.insert("front.camber_deg".into(), -2.3);
        let rear_spring = setup_items(&car, &tf, &tr)
            .into_iter()
            .find(|i| i.key == "rear.spring_n_per_mm")
            .unwrap()
            .default;
        setup
            .values
            .insert("rear.spring_n_per_mm".into(), (1.2 * rear_spring).round());
        setup.values.insert("brake_bias_pct".into(), 70.0);
        let sheet = setup_sheet(&car, &tf, &tr, &setup, "ridgeline_circuit").unwrap();
        assert_eq!(
            Setup::from_text(&sheet).unwrap(),
            setup,
            "{car_id}:\n{sheet}"
        );
        // The changed values show the car's own beside them.
        assert!(sheet.contains("-2.3 deg   (car:"), "{sheet}");
        // A sheet for the car as delivered is an empty setup.
        let plain = setup_sheet(&car, &tf, &tr, &Setup::default(), "").unwrap();
        assert_eq!(Setup::from_text(&plain).unwrap(), Setup::default());
    }
}

#[test]
fn the_setup_summary_is_physically_plausible() {
    use qts_sim::setup::setup_summary;
    let root = data();
    for car_id in CARS {
        let car = root.load_car(car_id).unwrap().value;
        let tf = root.load_tire(&car.tire_front).unwrap().value;
        let tr = root.load_tire(&car.tire_rear).unwrap().value;
        let lines = setup_summary(&car, &tf, &tr, &Setup::default()).unwrap();
        let find = |label: &str| {
            lines
                .iter()
                .find(|(l, _)| l == label)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| panic!("{car_id}: no {label}"))
        };
        let first_number = |text: String| -> f64 {
            text.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                .find(|t| !t.is_empty())
                .unwrap()
                .parse()
                .unwrap()
        };
        // Road cars ride at about 1 to 2 Hz.
        for axle in ["Front", "Rear"] {
            let hz = first_number(find(&format!("{axle} ride frequency")));
            assert!((1.0..=2.5).contains(&hz), "{car_id} {axle}: {hz} Hz");
        }
        let split = first_number(find("Roll stiffness at the front"));
        assert!((30.0..=75.0).contains(&split), "{car_id}: {split} %");
    }
    // The clutch pack's locking matches Drexler's lock-up table (D-107):
    // about 60 percent at 40 degrees and 42 at 50.
    let car = root.load_car("hatch_tern_gt_lsd").unwrap().value;
    let qts_vehicle::params::DrivetrainParams::EngineManual(m) = &car.drivetrain else {
        panic!("engine car");
    };
    let d = &m.differential;
    for (deg, drexler) in [
        (30.0, 0.88),
        (40.0, 0.60),
        (45.0, 0.51),
        (50.0, 0.42),
        (60.0, 0.29),
    ] {
        let lock = d.lock_per_input(deg);
        assert!(
            (lock - drexler).abs() < 0.02,
            "{deg} deg: {lock:.3} against {drexler}"
        );
    }
}
