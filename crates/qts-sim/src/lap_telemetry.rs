// SPDX-License-Identifier: Apache-2.0
//! Per-lap telemetry: every lap driven is written to its own CSV file in
//! the ordinary telemetry format (`docs/TELEMETRY_FORMAT.md`), and
//! [`compare`] lines two laps up by distance.
//!
//! The recorder only reads the session after each tick, like the
//! telemetry writer; it changes nothing in the simulation. Files are
//! written on a background thread so the physics thread never waits on
//! the disk.

use crate::session::Session;
use crate::telemetry;
use qts_core::input::InputFrame;
use qts_core::step::Simulation;
use qts_ffb::FfbSample;
use qts_telemetry::{TelemetryFile, TelemetryWriter};
use std::fmt::Write as _;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

/// Rows per second in a lap file.
pub const LAP_SAMPLE_RATE_HZ: f64 = 100.0;

/// Collects the rows of the lap in progress and writes each finished lap.
#[derive(Debug)]
pub struct LapRecorder {
    dir: PathBuf,
    decimation: u32,
    counter: u32,
    metadata: Vec<(String, String)>,
    rows: Vec<Vec<f64>>,
    row: Vec<f64>,
    seen: Option<(u32, u32)>,
    laps_written: u32,
    writers: Vec<JoinHandle<Result<PathBuf, String>>>,
    last: Option<Result<PathBuf, String>>,
}

impl LapRecorder {
    /// Writes laps of `session` into `dir`, which is created if needed.
    pub fn new(dir: &Path, session: &Session) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let rate = f64::from(session.tick_rate_hz());
        let decimation = (rate / LAP_SAMPLE_RATE_HZ).round().max(1.0) as u32;
        Ok(Self {
            dir: dir.to_path_buf(),
            decimation,
            counter: 0,
            metadata: telemetry::metadata(session, rate / f64::from(decimation)),
            rows: Vec::new(),
            row: Vec::new(),
            seen: None,
            laps_written: 0,
            writers: Vec::new(),
            last: None,
        })
    }

    /// Call after every tick.
    pub fn observe(&mut self, session: &Session, frame: &InputFrame, ffb: &FfbSample) {
        let t = session.timing();
        let finished = (t.runs, t.invalid_runs);
        if let Some(before) = self.seen {
            if finished != before && !self.rows.is_empty() {
                // The lap just ended: counted when `runs` went up.
                let counted = finished.0 > before.0;
                let ticks = if counted { t.last_ticks } else { 0 };
                self.flush(session, counted, ticks);
            }
        }
        self.seen = Some(finished);
        if !t.running {
            self.rows.clear();
            self.counter = 0;
            return;
        }
        if self.counter == 0 {
            telemetry::fill_row(session, frame, ffb, &mut self.row);
            self.rows.push(self.row.clone());
        }
        self.counter = (self.counter + 1) % self.decimation;
    }

    fn flush(&mut self, session: &Session, counted: bool, lap_ticks: u64) {
        self.laps_written += 1;
        let lap_s = lap_ticks as f64 / f64::from(session.tick_rate_hz());
        let name = if counted {
            format!(
                "lap_{:03}_{}m{:06.3}s.csv",
                self.laps_written,
                (lap_s / 60.0).floor(),
                lap_s % 60.0
            )
        } else {
            format!("lap_{:03}_not_counted.csv", self.laps_written)
        };
        let path = self.dir.join(name);
        let mut metadata = self.metadata.clone();
        metadata.push(("lap".into(), self.laps_written.to_string()));
        metadata.push(("lap_counted".into(), counted.to_string()));
        metadata.push((
            "lap_time_s".into(),
            if counted {
                format!("{lap_s:.3}")
            } else {
                String::new()
            },
        ));
        let rows = std::mem::take(&mut self.rows);
        self.counter = 0;
        self.writers.push(std::thread::spawn(move || {
            let file = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut w = TelemetryWriter::new(
                BufWriter::new(file),
                &telemetry::generator(),
                &metadata,
                &telemetry::channels(),
                1,
            )
            .map_err(|e| e.to_string())?;
            for r in &rows {
                w.sample(r).map_err(|e| e.to_string())?;
            }
            w.finish().map_err(|e| e.to_string())?;
            Ok(path)
        }));
    }

    /// Laps handed to the writer so far.
    pub fn laps_written(&self) -> u32 {
        self.laps_written
    }

    /// Collects finished writes; returns the newest result, if any.
    pub fn poll(&mut self) -> Option<&Result<PathBuf, String>> {
        let mut i = 0;
        while i < self.writers.len() {
            if self.writers[i].is_finished() {
                let h = self.writers.remove(i);
                self.last = Some(
                    h.join()
                        .unwrap_or_else(|_| Err("lap writer panicked".into())),
                );
            } else {
                i += 1;
            }
        }
        self.last.as_ref()
    }

    /// Waits for every pending write.
    pub fn wait(&mut self) -> Vec<Result<PathBuf, String>> {
        self.writers
            .drain(..)
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("lap writer panicked".into()))
            })
            .collect()
    }
}

/// One lap read back, with distance from the start of the lap.
#[derive(Clone, Debug)]
pub struct Lap {
    /// Label for reports.
    pub label: String,
    /// The file.
    pub file: TelemetryFile,
    /// Distance travelled since the first row, m (speed integrated).
    pub distance: Vec<f64>,
    /// Time since the first row, s.
    pub time: Vec<f64>,
}

impl Lap {
    /// Reads a lap file.
    pub fn read(path: &Path) -> Result<Self, String> {
        let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file = qts_telemetry::read_telemetry(std::io::BufReader::new(f))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_file(
            path.file_name()
                .map_or("lap".into(), |n| n.to_string_lossy().to_string()),
            file,
        )
    }

    /// Builds a lap from a parsed file.
    pub fn from_file(label: String, file: TelemetryFile) -> Result<Self, String> {
        let t = file.column("time_s").ok_or("no time_s column")?;
        let v = file.column("speed_m_s").ok_or("no speed_m_s column")?;
        if t.len() < 2 {
            return Err(format!("{label}: too few rows"));
        }
        let mut distance = vec![0.0; t.len()];
        for i in 1..t.len() {
            distance[i] = distance[i - 1] + 0.5 * (v[i] + v[i - 1]) * (t[i] - t[i - 1]);
        }
        let time = t.iter().map(|x| x - t[0]).collect();
        Ok(Self {
            label,
            file,
            distance,
            time,
        })
    }

    /// A column interpolated at a distance.
    pub fn at(&self, column: &[f64], d: f64) -> f64 {
        let i = self.distance.partition_point(|x| *x < d);
        if i == 0 {
            return column[0];
        }
        if i >= self.distance.len() {
            return column[column.len() - 1];
        }
        let (d0, d1) = (self.distance[i - 1], self.distance[i]);
        let f = if d1 > d0 { (d - d0) / (d1 - d0) } else { 0.0 };
        column[i - 1] + f * (column[i] - column[i - 1])
    }

    /// Total distance, m.
    pub fn length(&self) -> f64 {
        *self.distance.last().unwrap_or(&0.0)
    }
}

/// Channels drawn by [`compare`], as `(column, label, unit)`.
pub const COMPARE_CHANNELS: [(&str, &str, &str); 5] = [
    ("speed_m_s", "Speed", "km/h"),
    ("throttle", "Throttle", "%"),
    ("brake", "Brake", "%"),
    ("steering_wheel_rad", "Steering wheel", "deg"),
    ("steering_torque_nm", "Steering torque", "N.m"),
];

/// Two laps on a common distance grid and the time between them.
#[derive(Clone, Debug)]
pub struct Comparison {
    /// Distance grid, m.
    pub distance: Vec<f64>,
    /// Per channel in [`COMPARE_CHANNELS`]: lap A and lap B values, in
    /// display units.
    pub channels: Vec<(Vec<f64>, Vec<f64>)>,
    /// Time of B minus time of A at each distance, s (positive: B behind).
    pub delta: Vec<f64>,
}

fn display(column: &str, v: f64) -> f64 {
    match column {
        "speed_m_s" => v * 3.6,
        "throttle" | "brake" => v * 100.0,
        "steering_wheel_rad" => v.to_degrees(),
        _ => v,
    }
}

/// Lines `b` up against `a` by distance, every `step_m` metres, over the
/// shorter of the two laps.
pub fn compare(a: &Lap, b: &Lap, step_m: f64) -> Result<Comparison, String> {
    let length = a.length().min(b.length());
    if length <= step_m {
        return Err("laps too short to compare".into());
    }
    let n = (length / step_m).floor() as usize + 1;
    let distance: Vec<f64> = (0..n).map(|i| i as f64 * step_m).collect();
    let mut channels = Vec::new();
    for (col, _, _) in COMPARE_CHANNELS {
        let ca = a.file.column(col).ok_or(format!("{}: no {col}", a.label))?;
        let cb = b.file.column(col).ok_or(format!("{}: no {col}", b.label))?;
        channels.push((
            distance
                .iter()
                .map(|d| display(col, a.at(&ca, *d)))
                .collect(),
            distance
                .iter()
                .map(|d| display(col, b.at(&cb, *d)))
                .collect(),
        ));
    }
    let delta = distance
        .iter()
        .map(|d| b.at(&b.time, *d) - a.at(&a.time, *d))
        .collect();
    Ok(Comparison {
        distance,
        channels,
        delta,
    })
}

/// The comparison as an SVG chart: one panel per channel, lap A grey and
/// lap B orange, and the time delta at the bottom.
pub fn compare_svg(a: &Lap, b: &Lap, c: &Comparison) -> String {
    let (w, ml, mr, ph, gap) = (1100.0, 80.0, 20.0, 120.0, 34.0);
    let pw = w - ml - mr;
    let panels = COMPARE_CHANNELS.len() + 1;
    let h = 70.0 + panels as f64 * (ph + gap) + 30.0;
    let length = *c.distance.last().unwrap_or(&1.0);
    let x = |d: f64| ml + d / length * pw;
    let mut s = String::new();
    let _ = write!(
        s,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="Segoe UI, Arial, sans-serif"><rect width="{w}" height="{h}" fill="#ffffff"/>"##
    );
    let time = |l: &Lap| {
        l.file
            .meta("lap_time_s")
            .filter(|t| !t.is_empty())
            .map_or("not counted".to_string(), |t| format!("{t} s"))
    };
    let _ = write!(
        s,
        r##"<text x="{ml}" y="28" font-size="16" font-weight="700" fill="#111">Lap comparison by distance</text><text x="{ml}" y="50" font-size="13" fill="#444"><tspan fill="#8a8f98">A: {} ({})</tspan>   <tspan fill="#d9480f">B: {} ({})</tspan></text>"##,
        a.label,
        time(a),
        b.label,
        time(b)
    );
    let mut draw = |k: usize, title: &str, series: &[(&[f64], &str)]| {
        let y0 = 70.0 + k as f64 * (ph + gap) + 18.0;
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for (v, _) in series {
            for x in v.iter().filter(|x| x.is_finite()) {
                lo = lo.min(*x);
                hi = hi.max(*x);
            }
        }
        if hi - lo < 1e-9 {
            hi = lo + 1.0;
        }
        let y = |v: f64| y0 + ph - (v - lo) / (hi - lo) * ph;
        let _ = write!(
            s,
            r##"<text x="{ml}" y="{:.1}" font-size="13" font-weight="600" fill="#222">{title}</text><rect x="{ml}" y="{y0}" width="{pw}" height="{ph}" fill="none" stroke="#c9ccd1"/><text x="{:.1}" y="{:.1}" text-anchor="end" font-size="11" fill="#555">{hi:.1}</text><text x="{:.1}" y="{:.1}" text-anchor="end" font-size="11" fill="#555">{lo:.1}</text>"##,
            y0 - 6.0,
            ml - 6.0,
            y0 + 10.0,
            ml - 6.0,
            y0 + ph
        );
        if lo < 0.0 && hi > 0.0 {
            let _ = write!(
                s,
                r##"<line x1="{ml}" x2="{:.1}" y1="{:.1}" y2="{:.1}" stroke="#e3e5e8"/>"##,
                ml + pw,
                y(0.0),
                y(0.0)
            );
        }
        for (v, colour) in series {
            let pts: Vec<String> = c
                .distance
                .iter()
                .zip(v.iter())
                .map(|(d, v)| format!("{:.1},{:.1}", x(*d), y(*v)))
                .collect();
            let _ = write!(
                s,
                r##"<polyline points="{}" fill="none" stroke="{colour}" stroke-width="1.4"/>"##,
                pts.join(" ")
            );
        }
    };
    for (k, ((_, label, unit), (va, vb))) in COMPARE_CHANNELS.iter().zip(&c.channels).enumerate() {
        draw(
            k,
            &format!("{label} ({unit})"),
            &[(va, "#8a8f98"), (vb, "#d9480f")],
        );
    }
    draw(
        COMPARE_CHANNELS.len(),
        "Time of B minus A (s): above zero, B is behind",
        &[(&c.delta, "#1f6feb")],
    );
    let yb = h - 14.0;
    for i in 0..=10 {
        let d = length * i as f64 / 10.0;
        let _ = write!(
            s,
            r##"<text x="{:.1}" y="{yb}" text-anchor="middle" font-size="11" fill="#555">{d:.0} m</text>"##,
            x(d)
        );
    }
    s.push_str("</svg>");
    s
}
