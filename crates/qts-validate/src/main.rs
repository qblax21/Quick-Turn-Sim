// SPDX-License-Identifier: Apache-2.0
//! Validation harness binary: runs every target set, or the ones named on
//! the command line, prints the reports and exits non-zero if any target is
//! outside its tolerance.

use qts_core::constants::PRODUCT_NAME;
use qts_sim::DataRoot;
use qts_validate::{validate, TargetSet};
use std::process::ExitCode;

fn main() -> ExitCode {
    let requested: Vec<String> = std::env::args().skip(1).collect();
    if requested.iter().any(|a| a == "--help" || a == "-h") {
        println!("{PRODUCT_NAME} validation harness");
        println!("usage: qts-validate [TARGET_SET ...]");
        println!("With no arguments, every file in data/validation is run.");
        return ExitCode::SUCCESS;
    }
    let root = match DataRoot::discover() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ids = if requested.is_empty() {
        match root.list("validation") {
            Ok(ids) => ids,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        requested
    };
    if ids.is_empty() {
        eprintln!("error: no target sets found in data/validation");
        return ExitCode::FAILURE;
    }
    let mut all_passed = true;
    for id in &ids {
        match TargetSet::load(&root, id).and_then(|set| validate(&root, &set)) {
            Ok(report) => {
                println!("{report}\n");
                all_passed &= report.passed();
            }
            Err(e) => {
                eprintln!("error: {id}: {e}\n");
                all_passed = false;
            }
        }
    }
    if all_passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
