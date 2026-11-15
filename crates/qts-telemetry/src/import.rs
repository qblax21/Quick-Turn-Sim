// SPDX-License-Identifier: Apache-2.0
//! Import of CSV exports from data loggers.
//!
//! Loggers disagree on column names, units and sign conventions, so the
//! importer is driven by a profile: a small TOML file that says which
//! column feeds each canonical channel and how to convert it to SI. The
//! result is a run in canonical channels, which [`summarize`] turns into
//! the quantities the validation harness uses.
//!
//! Canonical channels (all SI):
//!
//! | Name | Meaning |
//! | --- | --- |
//! | `time_s` | Time, seconds. Required. |
//! | `speed_m_s` | Speed over the ground. |
//! | `accel_lat_m_s2` | Lateral acceleration, positive to the left. |
//! | `accel_long_m_s2` | Longitudinal acceleration, positive forward. |
//! | `yaw_rate_rad_s` | Yaw rate, positive counter-clockwise. |
//! | `steering_wheel_rad` | Steering-wheel angle, positive left. |
//! | `throttle` | Accelerator, 0 to 1. |
//! | `brake` | Brake pedal, 0 to 1. |
//! | `engine_rpm` | Engine speed, rpm. |
//! | `distance_m` | Distance travelled. |
//! | `latitude_deg`, `longitude_deg` | Position. |

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;

/// 60 mph in m/s.
pub const SIXTY_MPH: f64 = 26.8224;
/// Standard gravity, m/s².
const G: f64 = 9.806_65;

/// Canonical channel names the importer accepts.
pub const CANONICAL_CHANNELS: [&str; 12] = [
    "time_s",
    "speed_m_s",
    "accel_lat_m_s2",
    "accel_long_m_s2",
    "yaw_rate_rad_s",
    "steering_wheel_rad",
    "throttle",
    "brake",
    "engine_rpm",
    "distance_m",
    "latitude_deg",
    "longitude_deg",
];

fn default_delimiter() -> String {
    ",".to_string()
}

fn default_scale() -> f64 {
    1.0
}

/// How to read one logger's CSV export.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ImportProfile {
    /// Profile name.
    pub name: String,
    /// What the profile is for.
    #[serde(default)]
    pub description: String,
    /// Field separator, one character.
    #[serde(default = "default_delimiter")]
    pub delimiter: String,
    /// Lines to skip before the header row (titles, export metadata).
    #[serde(default)]
    pub skip_lines: usize,
    /// Lines to skip between the header row and the first data row (a units
    /// row, for example).
    #[serde(default)]
    pub skip_after_header: usize,
    /// Lines starting with this text are ignored wherever they appear.
    #[serde(default)]
    pub comment_prefix: Option<String>,
    /// Column mappings. Several entries may feed the same canonical
    /// channel; the first whose column exists in the file wins.
    #[serde(rename = "channel")]
    pub channels: Vec<ChannelMap>,
}

/// Mapping from a logger column to a canonical channel.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ChannelMap {
    /// Canonical channel this entry feeds.
    pub name: String,
    /// Header names to look for, compared without regard to case or
    /// surrounding spaces and quotes.
    pub columns: Vec<String>,
    /// Multiplier to SI.
    #[serde(default = "default_scale")]
    pub scale: f64,
    /// Offset added after scaling.
    #[serde(default)]
    pub offset: f64,
}

/// Error from importing a logger file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// The profile is unusable.
    Profile(String),
    /// The file has no header row where the profile expects one.
    NoHeader,
    /// No column matches the required time channel.
    NoTimeColumn(Vec<String>),
    /// The file has a header but no usable data rows.
    NoData,
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Profile(m) => write!(f, "import profile: {m}"),
            ImportError::NoHeader => write!(f, "no header row found"),
            ImportError::NoTimeColumn(found) => write!(
                f,
                "no column matches `time_s`; the file's columns are: {}",
                found.join(", ")
            ),
            ImportError::NoData => write!(f, "no usable data rows"),
        }
    }
}

impl std::error::Error for ImportError {}

/// A logger run converted to canonical channels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportedRun {
    /// Where the data came from (usually the file name).
    pub source: String,
    /// Canonical channel name to samples. Every channel has the same
    /// length; a field that could not be read is NaN.
    pub channels: BTreeMap<String, Vec<f64>>,
    /// Which logger column fed each canonical channel.
    pub mapped_from: BTreeMap<String, String>,
    /// Data rows kept.
    pub rows: usize,
    /// Data rows dropped because the time field was missing, unreadable or
    /// not increasing.
    pub skipped_rows: usize,
}

impl ImportedRun {
    /// Samples of a channel, if it was present in the file.
    pub fn channel(&self, name: &str) -> Option<&[f64]> {
        self.channels.get(name).map(|v| v.as_slice())
    }
}

/// Splits one CSV line, honouring double-quoted fields.
fn split_line(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' {
            quoted = true;
        } else if c == delimiter {
            fields.push(std::mem::take(&mut field));
        } else {
            field.push(c);
        }
    }
    fields.push(field);
    fields
}

fn normalise(name: &str) -> String {
    name.trim().trim_matches('"').trim().to_lowercase()
}

/// Reads logger CSV text using `profile`.
pub fn import_csv(
    text: &str,
    profile: &ImportProfile,
    source: &str,
) -> Result<ImportedRun, ImportError> {
    let mut delimiter_chars = profile.delimiter.chars();
    let delimiter = match (delimiter_chars.next(), delimiter_chars.next()) {
        (Some(c), None) => c,
        _ => {
            return Err(ImportError::Profile(
                "`delimiter` must be exactly one character".into(),
            ))
        }
    };
    for map in &profile.channels {
        if !CANONICAL_CHANNELS.contains(&map.name.as_str()) {
            return Err(ImportError::Profile(format!(
                "`{}` is not a canonical channel; use one of: {}",
                map.name,
                CANONICAL_CHANNELS.join(", ")
            )));
        }
        if !map.scale.is_finite() || !map.offset.is_finite() {
            return Err(ImportError::Profile(format!(
                "channel `{}` has a non-finite scale or offset",
                map.name
            )));
        }
    }
    let mut lines = text
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| match &profile.comment_prefix {
            Some(prefix) if !prefix.is_empty() => !l.starts_with(prefix.as_str()),
            _ => true,
        })
        .skip(profile.skip_lines);
    let header_line = lines
        .by_ref()
        .find(|l| !l.trim().is_empty())
        .ok_or(ImportError::NoHeader)?;
    let header: Vec<String> = split_line(header_line, delimiter)
        .iter()
        .map(|h| normalise(h))
        .collect();

    // Resolve each canonical channel to a column index.
    let mut resolved: Vec<(String, usize, f64, f64)> = Vec::new();
    let mut mapped_from = BTreeMap::new();
    for map in &profile.channels {
        if resolved.iter().any(|r| r.0 == map.name) {
            continue;
        }
        for candidate in &map.columns {
            let wanted = normalise(candidate);
            if let Some(index) = header.iter().position(|h| *h == wanted) {
                resolved.push((map.name.clone(), index, map.scale, map.offset));
                mapped_from.insert(map.name.clone(), candidate.clone());
                break;
            }
        }
    }
    let time_slot = resolved
        .iter()
        .position(|r| r.0 == "time_s")
        .ok_or_else(|| ImportError::NoTimeColumn(header.clone()))?;

    let mut columns: Vec<Vec<f64>> = vec![Vec::new(); resolved.len()];
    let mut skipped = 0usize;
    let mut last_time = f64::NEG_INFINITY;
    for line in lines.skip(profile.skip_after_header) {
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_line(line, delimiter);
        let read = |slot: usize| -> f64 {
            let (_, index, scale, offset) = &resolved[slot];
            fields
                .get(*index)
                .and_then(|f| f.trim().parse::<f64>().ok())
                .map_or(f64::NAN, |v| v * scale + offset)
        };
        let t = read(time_slot);
        if !t.is_finite() || t <= last_time {
            skipped += 1;
            continue;
        }
        last_time = t;
        for (slot, column) in columns.iter_mut().enumerate() {
            column.push(read(slot));
        }
    }
    let rows = columns[time_slot].len();
    if rows == 0 {
        return Err(ImportError::NoData);
    }
    let mut channels = BTreeMap::new();
    for ((name, _, _, _), column) in resolved.into_iter().zip(columns) {
        channels.insert(name, column);
    }
    Ok(ImportedRun {
        source: source.to_string(),
        channels,
        mapped_from,
        rows,
        skipped_rows: skipped,
    })
}

/// Headline numbers from a run. A field is `None` when the run does not
/// contain the channel or the manoeuvre needed to compute it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunSummary {
    /// Length of the run, s.
    pub duration_s: f64,
    /// Median sample rate, Hz.
    pub sample_rate_hz: f64,
    /// Highest speed, m/s.
    pub max_speed_m_s: Option<f64>,
    /// Highest lateral acceleration held for one second, in g.
    pub lateral_g_sustained: Option<f64>,
    /// Highest deceleration held for half a second, in g.
    pub braking_g_sustained: Option<f64>,
    /// Shortest standing start to 60 mph, s.
    pub zero_to_sixty_s: Option<f64>,
    /// Shortest stop from 60 mph, m.
    pub sixty_to_zero_m: Option<f64>,
}

/// Largest mean of `values` over any window of `window_s` seconds.
fn best_window_mean(time: &[f64], values: &[f64], window_s: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    let mut start = 0usize;
    let mut sum = 0.0;
    let mut count = 0usize;
    for end in 0..time.len() {
        if !values[end].is_finite() {
            // A gap breaks the window.
            start = end + 1;
            sum = 0.0;
            count = 0;
            continue;
        }
        sum += values[end];
        count += 1;
        while time[end] - time[start] > window_s {
            sum -= values[start];
            count -= 1;
            start += 1;
        }
        // Only count windows that really span the requested time.
        let spans =
            start > 0 && time[end] - time[start - 1] >= window_s && values[start - 1].is_finite();
        if count > 1 && (spans || time[end] - time[start] >= 0.95 * window_s) {
            let mean = sum / count as f64;
            if best.is_none_or(|b| mean > b) {
                best = Some(mean);
            }
        }
    }
    best
}

/// Computes the headline numbers of a run.
pub fn summarize(run: &ImportedRun) -> RunSummary {
    let time = match run.channel("time_s") {
        Some(t) if t.len() >= 2 => t,
        _ => return RunSummary::default(),
    };
    let mut steps: Vec<f64> = time.windows(2).map(|w| w[1] - w[0]).collect();
    steps.sort_by(|a, b| a.total_cmp(b));
    let median_step = steps[steps.len() / 2];
    let mut summary = RunSummary {
        duration_s: time[time.len() - 1] - time[0],
        sample_rate_hz: if median_step > 0.0 {
            1.0 / median_step
        } else {
            0.0
        },
        ..RunSummary::default()
    };

    if let Some(lat) = run.channel("accel_lat_m_s2") {
        let magnitude: Vec<f64> = lat.iter().map(|a| a.abs()).collect();
        summary.lateral_g_sustained = best_window_mean(time, &magnitude, 1.0).map(|a| a / G);
    }
    if let Some(long) = run.channel("accel_long_m_s2") {
        let decel: Vec<f64> = long.iter().map(|a| -a).collect();
        summary.braking_g_sustained = best_window_mean(time, &decel, 0.5)
            .filter(|a| *a > 0.0)
            .map(|a| a / G);
    }
    if let Some(speed) = run.channel("speed_m_s") {
        summary.max_speed_m_s = speed
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .reduce(f64::max);

        // Standing start: last time at rest before each upward crossing of
        // 60 mph, with speed never falling by more than 1 m/s on the way.
        const REST: f64 = 0.5;
        let mut rest_index: Option<usize> = None;
        let mut peak_since_rest = 0.0f64;
        for i in 0..speed.len() {
            let v = speed[i];
            if !v.is_finite() {
                rest_index = None;
                continue;
            }
            if v <= REST {
                rest_index = Some(i);
                peak_since_rest = v;
                continue;
            }
            if let Some(r) = rest_index {
                if v < peak_since_rest - 1.0 {
                    rest_index = None;
                    continue;
                }
                peak_since_rest = peak_since_rest.max(v);
                if v >= SIXTY_MPH && i > 0 && speed[i - 1] < SIXTY_MPH {
                    // Interpolate the crossing time.
                    let frac = (SIXTY_MPH - speed[i - 1]) / (v - speed[i - 1]);
                    let t_cross = time[i - 1] + frac * (time[i] - time[i - 1]);
                    // The last sample "at rest" may already be creeping at
                    // up to the rest threshold. Extend the launch back to
                    // zero speed using the acceleration over the next half
                    // second, by at most one second.
                    let mut k = r + 1;
                    while k < i && time[k] - time[r] < 0.5 {
                        k += 1;
                    }
                    let accel = (speed[k] - speed[r]) / (time[k] - time[r]);
                    let creep = if accel > 0.0 {
                        (speed[r] / accel).min(1.0)
                    } else {
                        0.0
                    };
                    let elapsed = t_cross - (time[r] - creep);
                    if summary.zero_to_sixty_s.is_none_or(|b| elapsed < b) {
                        summary.zero_to_sixty_s = Some(elapsed);
                    }
                    rest_index = None;
                }
            }
        }

        // Stop from 60 mph: distance from the downward crossing to rest,
        // with speed never rising by more than 1 m/s on the way.
        let mut i = 1;
        while i < speed.len() {
            if speed[i - 1] >= SIXTY_MPH && speed[i] < SIXTY_MPH {
                let frac = (speed[i - 1] - SIXTY_MPH) / (speed[i - 1] - speed[i]);
                let mut t_prev = time[i - 1] + frac * (time[i] - time[i - 1]);
                let mut v_prev = SIXTY_MPH;
                let mut distance = 0.0;
                let mut lowest = SIXTY_MPH;
                let mut j = i;
                let mut stopped = false;
                while j < speed.len() {
                    let v = speed[j];
                    if !v.is_finite() || v > lowest + 1.0 {
                        break;
                    }
                    lowest = lowest.min(v);
                    distance += 0.5 * (v_prev + v) * (time[j] - t_prev);
                    t_prev = time[j];
                    v_prev = v;
                    if v <= REST {
                        stopped = true;
                        break;
                    }
                    j += 1;
                }
                if stopped && summary.sixty_to_zero_m.is_none_or(|b| distance < b) {
                    summary.sixty_to_zero_m = Some(distance);
                }
                i = j.max(i + 1);
            } else {
                i += 1;
            }
        }
    }
    summary
}

/// Formats the measured values of a summary as entries for a validation
/// target file, tagged `confidence = "measured"` with the source name.
/// Tolerances are left for the user to choose; the snippet uses 5 percent.
pub fn targets_snippet(summary: &RunSummary, source: &str) -> String {
    let mut out = String::new();
    let mut entry = |metric: &str, value: f64, unit: &str| {
        out.push_str(&format!(
            "[[target]]\nmetric = \"{metric}\"\nvalue = {value:.4}\nunit = \"{unit}\"\ntolerance_percent = 5.0\nsource = \"imported from {source}\"\nconfidence = \"measured\"\n\n"
        ));
    };
    if let Some(v) = summary.lateral_g_sustained {
        entry("skidpad_lateral_g", v, "g");
    }
    if let Some(v) = summary.sixty_to_zero_m {
        entry("braking_distance_60_0", v, "m");
    }
    if let Some(v) = summary.zero_to_sixty_s {
        entry("accel_0_60", v, "s");
    }
    if let Some(v) = summary.max_speed_m_s {
        entry("top_speed", v, "m/s");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(text: &str) -> ImportProfile {
        // Parsed by hand here to keep this crate free of a TOML dependency;
        // `qts-sim` loads real profiles from `data/import`.
        let mut channels = Vec::new();
        for line in text.lines() {
            let parts: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
            if parts.len() == 3 {
                channels.push(ChannelMap {
                    name: parts[0].to_string(),
                    columns: parts[1].split(';').map(|s| s.trim().to_string()).collect(),
                    scale: parts[2].parse().unwrap(),
                    offset: 0.0,
                });
            }
        }
        ImportProfile {
            name: "test".into(),
            description: String::new(),
            delimiter: ",".into(),
            skip_lines: 0,
            skip_after_header: 0,
            comment_prefix: Some("#".into()),
            channels,
        }
    }

    #[test]
    fn columns_are_matched_and_converted() {
        let p = profile(
            "time_s | Time; time (s) | 1.0\n\
             speed_m_s | Speed (km/h) | 0.2777777777777778\n\
             speed_m_s | Speed (mph) | 0.44704\n\
             accel_lat_m_s2 | Lat G | 9.80665",
        );
        let csv = "# exported by a logger\n\"Time (s)\",\"Speed (MPH)\",Lat G,Other\n0.0,10,0.5,x\n0.1,20,-0.25,y\nbad,30,0,z\n0.05,40,0,late\n0.2,,0.1,w\n";
        let run = import_csv(csv, &p, "run.csv").unwrap();
        assert_eq!(run.rows, 3);
        assert_eq!(run.skipped_rows, 2);
        assert_eq!(run.mapped_from["speed_m_s"], "Speed (mph)");
        let speed = run.channel("speed_m_s").unwrap();
        assert!((speed[0] - 4.4704).abs() < 1e-9 && (speed[1] - 8.9408).abs() < 1e-9);
        assert!(speed[2].is_nan());
        let lat = run.channel("accel_lat_m_s2").unwrap();
        assert!((lat[1] + 0.25 * 9.80665).abs() < 1e-9);
        assert!(run.channel("brake").is_none());
    }

    #[test]
    fn problems_are_reported_clearly() {
        let p = profile("time_s | Time | 1.0");
        assert_eq!(import_csv("", &p, "x"), Err(ImportError::NoHeader));
        match import_csv("a,b\n1,2\n", &p, "x") {
            Err(ImportError::NoTimeColumn(cols)) => assert_eq!(cols, ["a", "b"]),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            import_csv("Time,b\nx,2\n", &p, "x"),
            Err(ImportError::NoData)
        );
        let mut bad = p.clone();
        bad.channels[0].name = "velocity".into();
        assert!(matches!(
            import_csv("Time\n1\n", &bad, "x"),
            Err(ImportError::Profile(_))
        ));
        let mut bad = p;
        bad.delimiter = "::".into();
        assert!(matches!(
            import_csv("Time\n1\n", &bad, "x"),
            Err(ImportError::Profile(_))
        ));
    }

    #[test]
    fn quoted_fields_may_contain_the_delimiter() {
        assert_eq!(
            split_line("a,\"b,c\",\"d\"\"e\"", ','),
            ["a", "b,c", "d\"e"]
        );
        assert_eq!(split_line("1;2;;4", ';'), ["1", "2", "", "4"]);
    }

    /// Builds a run from closed-form motion so the expected summary values
    /// are known exactly.
    fn synthetic_run() -> ImportedRun {
        let dt = 0.02;
        let mut time = Vec::new();
        let mut speed = Vec::new();
        let mut lat = Vec::new();
        let mut long = Vec::new();
        let mut t = 0.0;
        let mut push = |t: &mut f64, v: f64, a_lat: f64, a_long: f64| {
            time.push(*t);
            speed.push(v);
            lat.push(a_lat);
            long.push(a_long);
            *t += dt;
        };
        // 1 s at rest.
        for _ in 0..50 {
            push(&mut t, 0.0, 0.0, 0.0);
        }
        // Constant 4 m/s² to 30 m/s: 60 mph is reached after 6.7056 s.
        let mut v = 0.0;
        while v < 30.0 {
            v += 4.0 * dt;
            push(&mut t, v, 0.0, 4.0);
        }
        // 3 s cornering at 0.9 g with a 0.2 s spike to 1.3 g.
        for i in 0..150 {
            let a = if (50..60).contains(&i) {
                1.3 * G
            } else {
                0.9 * G
            };
            push(&mut t, v, -a, 0.0);
        }
        // Constant 9 m/s² stop: from 60 mph that takes 26.8224² / 18 m.
        while v > 0.0 {
            v = (v - 9.0 * dt).max(0.0);
            push(&mut t, v, 0.0, -9.0);
        }
        for _ in 0..20 {
            push(&mut t, 0.0, 0.0, 0.0);
        }
        let mut run = ImportedRun {
            source: "synthetic".into(),
            rows: time.len(),
            ..ImportedRun::default()
        };
        run.channels.insert("time_s".into(), time);
        run.channels.insert("speed_m_s".into(), speed);
        run.channels.insert("accel_lat_m_s2".into(), lat);
        run.channels.insert("accel_long_m_s2".into(), long);
        run
    }

    #[test]
    fn summary_recovers_known_manoeuvres() {
        let s = summarize(&synthetic_run());
        assert!((s.sample_rate_hz - 50.0).abs() < 1e-6);
        assert!((s.max_speed_m_s.unwrap() - 30.0).abs() < 0.1);
        let t60 = s.zero_to_sixty_s.unwrap();
        assert!((t60 - SIXTY_MPH / 4.0).abs() < 0.03, "0-60 in {t60}");
        let d = s.sixty_to_zero_m.unwrap();
        let expected = SIXTY_MPH * SIXTY_MPH / 18.0;
        assert!(
            (d - expected).abs() < 0.02 * expected,
            "stop in {d} m, expected {expected}"
        );
        // The one-second window must not be fooled by the short spike:
        // 0.8 s at 0.9 g plus 0.2 s at 1.3 g averages 0.98 g.
        let lat = s.lateral_g_sustained.unwrap();
        assert!((lat - 0.98).abs() < 0.02, "sustained lateral {lat} g");
        let brake = s.braking_g_sustained.unwrap();
        assert!(
            (brake - 9.0 / G).abs() < 0.01,
            "sustained braking {brake} g"
        );
    }

    #[test]
    fn snippet_lists_only_what_was_measured() {
        let s = RunSummary {
            lateral_g_sustained: Some(0.912_34),
            zero_to_sixty_s: Some(6.5),
            ..RunSummary::default()
        };
        let text = targets_snippet(&s, "day1.csv");
        assert!(text.contains("metric = \"skidpad_lateral_g\"\nvalue = 0.9123\n"));
        assert!(text.contains("metric = \"accel_0_60\""));
        assert!(text.contains("confidence = \"measured\""));
        assert!(text.contains("imported from day1.csv"));
        assert!(!text.contains("top_speed") && !text.contains("braking_distance"));
    }
}
