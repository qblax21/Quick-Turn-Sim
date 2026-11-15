// SPDX-License-Identifier: Apache-2.0
//! The shipped data files: they load, they are honest about provenance,
//! and the three tracks can be driven and timed.

mod common;

use common::*;
use qts_core::input::{buttons, InputFrame};
use qts_core::provenance::Confidence;
use qts_core::step::Simulation;
use qts_ffb::{FfbPipeline, FfbSample};
use qts_sim::data::{provenance_report, DataError};
use qts_sim::drivers::{Driver, PathFollower, PathFollowerConfig};
use qts_sim::host::ffb_input;
use qts_sim::session::SessionAssets;
use qts_sim::telemetry;
use qts_telemetry::{read_telemetry, TelemetryWriter};

#[test]
fn every_data_file_loads() {
    let root = data();
    let cars = root.list("cars").unwrap();
    assert_eq!(cars, CARS);
    for id in &cars {
        let car = root.load_car(id).unwrap();
        root.load_tire(&car.value.tire_front).unwrap();
        root.load_tire(&car.value.tire_rear).unwrap();
        assert!(!car.provenance.is_empty());
    }
    let mut tracks = root.list("tracks").unwrap();
    tracks.sort();
    let mut expected = TRACKS.to_vec();
    expected.sort_unstable();
    assert_eq!(tracks, expected);
    for id in root.list("tires").unwrap() {
        root.load_tire(&id).unwrap();
    }
    assert_eq!(root.load_all_surfaces().unwrap().len(), 4);
    for id in root.list("ffb").unwrap() {
        let preset = root.load_ffb(&id).unwrap();
        FfbPipeline::new(preset.value, 1000).unwrap();
    }
    for id in root.list("import").unwrap() {
        root.load_import_profile(&id).unwrap();
    }
    for car in CARS {
        for track in TRACKS {
            SessionAssets::load(&root, car, track).unwrap();
        }
    }
}

#[test]
fn nothing_claims_to_be_measured_and_untuned_things_say_so() {
    let root = data();
    // No measurement has been supplied to the project yet. If a parameter
    // is ever marked `measured`, this test must be updated together with
    // the evidence for it.
    for car in CARS {
        let assets = SessionAssets::load(&root, car, "gravel_stage").unwrap();
        let measured: Vec<_> = assets
            .provenance
            .iter()
            .filter(|p| p.confidence == Confidence::Measured)
            .collect();
        assert!(
            measured.is_empty(),
            "unexpected measured parameters: {measured:?}"
        );
        assert!(
            assets.provenance.len() > 150,
            "{car}: {} parameters",
            assets.provenance.len()
        );
        let report = provenance_report(car, &assets.provenance);
        assert!(report.contains("0 measured"));
    }
    assert!(!root.load_surface("gravel").unwrap().value.tuned);
    assert!(!root.load_surface("dirt").unwrap().value.tuned);
    assert!(root.load_surface("tarmac").unwrap().value.tuned);
    let preset = root.load_ffb("fanatec_csl_elite").unwrap().value;
    assert!(!preset.tuned);
    assert!(preset.name.to_lowercase().contains("untuned"));
}

#[test]
fn the_limited_slip_variant_differs_only_in_its_differential() {
    let root = data();
    let base = root.load_car("hatch_tern_gt").unwrap().value;
    let lsd = root.load_car("hatch_tern_gt_lsd").unwrap().value;
    assert_eq!(base.body, lsd.body);
    assert_eq!(base.suspension, lsd.suspension);
    assert_ne!(base.drivetrain, lsd.drivetrain);
    assert_ne!(
        root.load_car("hatch_tern_gt").unwrap().content_hash,
        root.load_car("hatch_tern_gt_lsd").unwrap().content_hash
    );
}

#[test]
fn bad_identifiers_and_missing_files_are_reported() {
    let root = data();
    assert!(matches!(
        root.load_car("no_such_car"),
        Err(DataError::Io { .. })
    ));
    assert!(matches!(
        root.load_car("../cars/hatch_tern_gt"),
        Err(DataError::Invalid { .. })
    ));
    assert!(matches!(
        root.load_track("hatch_tern_gt"),
        Err(DataError::Io { .. })
    ));
}

#[test]
fn cars_are_fictional_and_names_come_from_one_place() {
    // The product name must not be written into data files: it lives in
    // qts-core's constants and the README only.
    let name = qts_core::constants::PRODUCT_NAME;
    let short = qts_core::constants::PRODUCT_SHORT_NAME;
    let root = repo_root().join("data");
    let mut stack = vec![root];
    let mut checked = 0;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let text = std::fs::read_to_string(&path).unwrap();
                assert!(
                    !text.contains(name),
                    "{} contains the product name",
                    path.display()
                );
                assert!(
                    !text
                        .split(|c: char| !c.is_ascii_alphanumeric())
                        .any(|w| w == short),
                    "{} contains the short product name",
                    path.display()
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 12);
    for car in CARS {
        let c = data().load_car(car).unwrap().value;
        assert!(
            c.class.to_lowercase().contains("fictional"),
            "{car} must be labelled fictional"
        );
    }
}

#[test]
fn the_autocross_is_timed_and_counts_cones() {
    let mut s = session("hatch_tern_gt", "autocross_lot");
    assert_eq!(s.track().cones().len(), 69);
    let path = s.track().path("course").unwrap().clone();
    let mut driver = PathFollower::new(&path, PathFollowerConfig::default());
    let mut saw_running = false;
    for _ in 0..60_000 {
        let frame = driver.frame(&s);
        s.step(&frame);
        saw_running |= s.timing().running;
        if s.timing().runs == 1 && s.car().output().speed < 0.2 {
            break;
        }
    }
    assert!(saw_running);
    let t = s.timing().clone();
    assert_eq!(t.runs, 1);
    let seconds = s.ticks_to_seconds(t.last_ticks);
    assert!((15.0..45.0).contains(&seconds), "run took {seconds} s");
    assert_eq!(t.best_ticks, t.last_ticks);
    assert_eq!(t.last_cones_hit, 0, "the scripted driver should run clean");

    // Park the car on a cone: it goes down once and is counted once.
    let cone = s.track().cones()[10];
    s.car_mut().place(cone[0], cone[1], 0.0, 0.0);
    for _ in 0..5 {
        s.step(&InputFrame::default());
    }
    assert!(s.timing().cones_hit >= 1);
    let after_first = s.timing().cones_hit;
    for _ in 0..200 {
        s.step(&InputFrame::default());
    }
    assert_eq!(s.timing().cones_hit, after_first);

    // Reset returns the car to the spawn point and stands the cones up.
    let reset = InputFrame::default().with_buttons(buttons::RESET);
    s.step(&reset);
    let (x, y, _) = s.track().spawn();
    assert!((s.car().state.pos.x - x).abs() < 0.01 && (s.car().state.pos.y - y).abs() < 0.01);
    assert_eq!(s.timing().cones_hit, 0);
    assert!(s.timing().cone_down.iter().all(|d| !d));
    // Holding the button does not reset again.
    s.step(&InputFrame::from_analog(0.0, 1.0, 0.0, 0.0, 0.0).with_buttons(buttons::RESET));
    for _ in 0..2000 {
        s.step(&InputFrame::from_analog(0.0, 1.0, 0.0, 0.0, 0.0).with_buttons(buttons::RESET));
    }
    assert!(
        s.car().state.pos.x > x + 1.0,
        "car should drive away while the button is held"
    );
}

#[test]
fn the_stage_changes_surface_and_both_cars_finish_it() {
    for car in ["hatch_tern_gt", "sedan_halden_e4"] {
        let mut s = session(car, "gravel_stage");
        let path = s.track().path("stage").unwrap().clone();
        assert!(
            (path.length() - 1000.0).abs() < 60.0,
            "stage is {} m",
            path.length()
        );
        let mut driver = PathFollower::new(&path, PathFollowerConfig::default());
        let mut surfaces_seen = Vec::new();
        for _ in 0..150_000 {
            let frame = driver.frame(&s);
            s.step(&frame);
            let index = s.car().output().wheels[0].surface_index;
            let id = s.track().surfaces()[usize::from(index)].id.clone();
            if surfaces_seen.last() != Some(&id) {
                surfaces_seen.push(id);
            }
            if s.timing().runs == 1 {
                break;
            }
        }
        assert_eq!(s.timing().runs, 1, "{car} did not finish the stage");
        assert_eq!(surfaces_seen, ["gravel", "tarmac"], "{car} left the road");
        let seconds = s.ticks_to_seconds(s.timing().last_ticks);
        assert!((40.0..120.0).contains(&seconds), "{car} took {seconds} s");
    }
}

#[test]
fn the_test_track_has_a_kerb_a_loose_patch_and_a_timed_strip() {
    let s = session("hatch_tern_gt", "test_track");
    let track = s.track();
    let id = |x: f64, y: f64| {
        track.surfaces()[usize::from(track.surface_at(x, y).0)]
            .id
            .clone()
    };
    assert_eq!(id(0.0, 0.0), "tarmac");
    assert_eq!(id(150.0, -6.0), "kerb");
    assert_eq!(id(150.0, 14.0), "gravel");
    assert!(
        track.surface_at(150.17, -6.0).1 > 0.01,
        "kerb ridges should have height"
    );
    assert_eq!(track.gates().len(), 2);
    // A quarter mile between the gates.
    let run = track.gates()[1].a[0] - track.gates()[0].a[0];
    assert!((run - 402.336).abs() < 1e-6);
}

#[test]
fn telemetry_rows_match_the_documented_channels() {
    let mut s = session("sedan_halden_e4", "test_track");
    let root = data();
    let mut pipeline =
        FfbPipeline::new(root.load_ffb("fanatec_csl_elite").unwrap().value, 1000).unwrap();
    let channels = telemetry::channels();
    let mut writer = TelemetryWriter::new(
        Vec::new(),
        &telemetry::generator(),
        &telemetry::metadata(&s, 100.0),
        &channels,
        10,
    )
    .unwrap();
    let mut row = Vec::new();
    let mut sample = FfbSample::default();
    for i in 0..3000 {
        let steer = if i > 1500 { 0.1 } else { 0.0 };
        let frame = InputFrame::from_analog(steer, 0.6, 0.0, 0.0, 0.0);
        s.step(&frame);
        sample = pipeline.step(&ffb_input(&s, None));
        telemetry::fill_row(&s, &frame, &sample, &mut row);
        assert_eq!(row.len(), channels.len());
        writer.sample(&row).unwrap();
    }
    assert!(sample.raw_nm.abs() > 0.1, "steering should load the wheel");
    let bytes = writer.finish().unwrap();
    let file = read_telemetry(bytes.as_slice()).unwrap();
    assert_eq!(file.rows.len(), 300);
    assert_eq!(file.meta("car"), Some("sedan_halden_e4"));
    assert!(file
        .meta("generator")
        .unwrap()
        .starts_with(qts_core::constants::PRODUCT_NAME));
    // Spot-check that values land in the right columns.
    let speed = file.column("speed_m_s").unwrap();
    assert!(speed[299] > speed[10] && speed[299] > 5.0);
    let time = file.column("time_s").unwrap();
    assert!((time[1] - time[0] - 0.01).abs() < 1e-6);
    let raw = file.column("ffb_raw_nm").unwrap();
    let torque = file.column("steering_torque_nm").unwrap();
    assert_eq!(
        raw, torque,
        "pre-filter FFB signal is the raw steering torque"
    );
    let output = file.column("ffb_output").unwrap();
    assert!(output.iter().all(|v| v.abs() <= 1.0));
    let load = file.column("fl_load_n").unwrap();
    assert!(load.iter().all(|v| *v > 500.0 && *v < 20_000.0));
    let soc = file.column("battery_soc").unwrap();
    assert!(soc[299] < soc[0]);

    // The format document must describe every channel.
    let doc = std::fs::read_to_string(repo_root().join("docs/TELEMETRY_FORMAT.md"))
        .expect("docs/TELEMETRY_FORMAT.md exists");
    for c in &channels {
        assert!(
            doc.contains(&format!("`{}`", c.name)),
            "docs/TELEMETRY_FORMAT.md does not document `{}`",
            c.name
        );
    }
}
