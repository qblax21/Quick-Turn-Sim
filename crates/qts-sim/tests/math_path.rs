// SPDX-License-Identifier: Apache-2.0
//! Every transcendental function in the simulation path goes through
//! `qts_core::math::fm`, which computes it in software. A call to the `f64`
//! method instead would use the platform math library, whose results differ
//! between binaries and platforms, and replays would stop reproducing.

use std::path::Path;

const FORBIDDEN: [&str; 16] = [
    ".sin(", ".cos(", ".tan(", ".asin(", ".acos(", ".atan(", ".atan2(", ".exp(", ".ln(", ".log10(",
    ".log2(", ".powf(", ".tanh(", ".sinh(", ".cosh(", ".hypot(",
];

fn check_file(path: &Path, problems: &mut Vec<String>) {
    let text = std::fs::read_to_string(path).unwrap();
    for (n, line) in text.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        // Unit tests at the end of a file are not in the simulation path.
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        for f in FORBIDDEN {
            if code.contains(f) {
                problems.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
            }
        }
    }
}

#[test]
fn the_simulation_path_uses_only_the_software_math_functions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in [
        "crates/qts-core/src",
        "crates/qts-tire/src",
        "crates/qts-vehicle/src",
    ] {
        let mut stack = vec![root.join(dir)];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).unwrap() {
                let p = entry.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|e| e == "rs") && !p.ends_with("math.rs") {
                    files.push(p);
                }
            }
        }
    }
    files.push(root.join("crates/qts-sim/src/session.rs"));
    files.push(root.join("crates/qts-sim/src/track.rs"));
    let mut problems = Vec::new();
    for f in &files {
        check_file(f, &mut problems);
    }
    assert!(
        problems.is_empty(),
        "use qts_core::math::fm instead:\n{}",
        problems.join("\n")
    );
}
