// SPDX-License-Identifier: Apache-2.0
//! Shared helpers for the session integration tests.
#![allow(dead_code)]

use qts_sim::session::{Session, SessionConfig};
use qts_sim::DataRoot;
use qts_vehicle::Assists;

pub const CARS: [&str; 3] = ["hatch_tern_gt", "hatch_tern_gt_lsd", "sedan_halden_e4"];
pub const TRACKS: [&str; 4] = [
    "autocross_lot",
    "test_track",
    "gravel_stage",
    "ridgeline_circuit",
];

/// The repository's `data/` directory.
pub fn data() -> DataRoot {
    DataRoot::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data"))
}

/// The repository root.
pub fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A session with both assists on, which is what the scripted drivers need.
pub fn session(car: &str, track: &str) -> Session {
    let config = SessionConfig {
        assists: Assists {
            auto_clutch: true,
            auto_shift: true,
            traction_control: true,
        },
        ..SessionConfig::default()
    };
    Session::from_data(&data(), car, track, config).expect("session loads")
}
