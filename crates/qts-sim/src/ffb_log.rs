// SPDX-License-Identifier: Apache-2.0
//! Force-feedback logs: one lap of the force-feedback signal at the physics
//! rate, before and after every stage of the pipeline, with the steering
//! torque the physics produced and the front tire state that explains it.
//!
//! A log is made for tuning a wheel preset. The file is plain CSV with `#`
//! comment lines at the top recording the car, track and every pipeline
//! setting, so a log is enough on its own to say what the driver felt and
//! why. [`report`] reads one back and summarises clipping and how the
//! torque behaves as the front tires reach their grip limit.
//!
//! Nothing here feeds back into the simulation.

use crate::session::Session;
use qts_core::constants::{CORE_VERSION, PRODUCT_NAME};
use qts_core::input::{InputFrame, AXIS_MAX};
use qts_core::step::Simulation;
use qts_ffb::{FfbConfig, FfbInput, FfbSample};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

/// Columns of a force-feedback log, with units and meaning.
pub const COLUMNS: [(&str, &str); 29] = [
    ("time_s", "simulated time since the session started"),
    ("run_time_s", "time since the lap started"),
    ("distance_m", "distance driven since the lap started"),
    ("speed_m_s", "speed over the ground"),
    (
        "steer_input",
        "steering input reaching the physics, -1 to 1, positive left",
    ),
    (
        "wheel_angle_rad",
        "steering-wheel angle the physics used, positive left",
    ),
    (
        "device_angle_rad",
        "angle the physical wheel reported, empty if none",
    ),
    (
        "column_torque_nm",
        "steering-column torque from the physics",
    ),
    (
        "kick_nm",
        "impact kick added on hitting a tree or barrier (an effect)",
    ),
    ("raw_nm", "pipeline input: column torque plus kick"),
    ("scaled_nm", "after gain, before filtering"),
    ("filtered_nm", "after the low-pass filter and slew limiter"),
    (
        "effects_nm",
        "damping, friction and inertia added at the wheel",
    ),
    ("detail_nm", "road-detail and kerb channels"),
    (
        "requested",
        "total asked of the device, fraction of its peak, may exceed 1",
    ),
    (
        "output",
        "sent to the device after soft clip and minimum force, -1 to 1",
    ),
    (
        "clipped",
        "how much the soft clip removed, fraction of peak",
    ),
    ("kerb_contact", "share of the front wheels on a kerb"),
    ("front_load_diff_n", "left minus right front tire load"),
    ("fl_slip_angle_rad", "front-left slip angle"),
    ("fr_slip_angle_rad", "front-right slip angle"),
    ("fl_fy_n", "front-left lateral force"),
    ("fr_fy_n", "front-right lateral force"),
    ("fl_mz_nm", "front-left aligning moment"),
    ("fr_mz_nm", "front-right aligning moment"),
    ("fl_load_n", "front-left vertical load"),
    ("fr_load_n", "front-right vertical load"),
    ("accel_lat_m_s2", "lateral acceleration, positive left"),
    ("accel_long_m_s2", "longitudinal acceleration"),
];

const NCOL: usize = COLUMNS.len();
/// Level of `clipped` above which a tick counts as clipping. Matches the
/// live meter.
pub const CLIP_THRESHOLD: f64 = 0.02;
/// Longest lap recorded, s. A log is abandoned past this.
const MAX_LAP_S: f64 = 900.0;

/// What a log in progress is doing, for the HUD.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum FfbLogStatus {
    /// No log asked for.
    #[default]
    Off,
    /// Waiting for the car to cross the start line.
    Waiting,
    /// Recording the lap, with seconds recorded so far.
    Recording(f64),
    /// Writing the file.
    Saving,
    /// Written. The path and whether the lap counted.
    Saved {
        /// File written.
        path: PathBuf,
        /// False when the lap missed part of the course; the log is still
        /// kept, since force feedback does not care.
        lap_counted: bool,
    },
    /// Could not be written.
    Failed(String),
}

enum Phase {
    Waiting,
    Recording { start_tick: u64 },
    Saving(JoinHandle<Result<(PathBuf, bool), String>>),
    Done(FfbLogStatus),
}

/// A log armed for the next lap. Owned by the sim thread.
pub struct FfbLogger {
    path: PathBuf,
    phase: Phase,
    rows: Vec<f64>,
    finished_before: u32,
    counted_before: u32,
    tick_rate_hz: f64,
}

impl FfbLogger {
    /// Arms a log that records the next full lap into `path`.
    pub fn arm(path: PathBuf, session: &Session) -> Self {
        let t = session.timing();
        Self {
            path,
            phase: Phase::Waiting,
            rows: Vec::new(),
            finished_before: t.runs + t.invalid_runs,
            counted_before: t.runs,
            tick_rate_hz: f64::from(session.tick_rate_hz()),
        }
    }

    /// Records one tick. Call after the session and pipeline stepped.
    pub fn observe(
        &mut self,
        session: &Session,
        frame: &InputFrame,
        input: &FfbInput,
        sample: &FfbSample,
        config: &FfbConfig,
    ) {
        let t = session.timing();
        let finished = t.runs + t.invalid_runs;
        match self.phase {
            Phase::Waiting => {
                // A lap starts on the tick the start line is crossed.
                if t.running && t.start_tick == session.tick() {
                    self.phase = Phase::Recording {
                        start_tick: t.start_tick,
                    };
                    self.rows.clear();
                    self.finished_before = finished;
                    self.counted_before = t.runs;
                    self.push_row(session, frame, input, sample);
                }
            }
            Phase::Recording { start_tick } => {
                if finished > self.finished_before {
                    // The finish line: this tick also starts the next lap on
                    // a circuit, so it belongs to neither and is not logged.
                    // An invalid lap moves `invalid_runs`, a counted one `runs`.
                    let counted = t.runs > self.counted_before;
                    self.save(session, config, counted);
                } else if !t.running || t.start_tick != start_tick {
                    // Reset or restarted: wait for a clean lap.
                    self.rows.clear();
                    self.phase = Phase::Waiting;
                } else if session.ticks_to_seconds(session.tick() - start_tick) > MAX_LAP_S {
                    self.phase = Phase::Done(FfbLogStatus::Failed(
                        "the lap took too long; nothing was saved".into(),
                    ));
                    self.rows = Vec::new();
                } else {
                    self.push_row(session, frame, input, sample);
                }
            }
            Phase::Saving(_) | Phase::Done(_) => {}
        }
    }

    /// Stops early. A lap being recorded is saved as far as it went.
    pub fn stop(&mut self, session: &Session, config: &FfbConfig) {
        if matches!(self.phase, Phase::Recording { .. }) && !self.rows.is_empty() {
            self.save(session, config, false);
        } else if !matches!(self.phase, Phase::Saving(_)) {
            self.phase = Phase::Done(FfbLogStatus::Off);
        }
    }

    /// Waits for a file being written and returns the final state.
    pub fn wait(&mut self) -> FfbLogStatus {
        while matches!(self.phase, Phase::Saving(_)) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.status();
        }
        self.status()
    }

    /// True once there is nothing left to do.
    pub fn is_done(&self) -> bool {
        matches!(self.phase, Phase::Done(_))
    }

    /// Current state, collecting the writer thread's result if it ended.
    pub fn status(&mut self) -> FfbLogStatus {
        if let Phase::Saving(handle) = &self.phase {
            if handle.is_finished() {
                let Phase::Saving(handle) = std::mem::replace(&mut self.phase, Phase::Waiting)
                else {
                    unreachable!()
                };
                let status = match handle.join() {
                    Ok(Ok((path, lap_counted))) => FfbLogStatus::Saved { path, lap_counted },
                    Ok(Err(e)) => FfbLogStatus::Failed(e),
                    Err(_) => FfbLogStatus::Failed("the log writer stopped unexpectedly".into()),
                };
                self.phase = Phase::Done(status);
            }
        }
        match &self.phase {
            Phase::Waiting => FfbLogStatus::Waiting,
            Phase::Recording { .. } => {
                let rows = self.rows.len() / NCOL;
                FfbLogStatus::Recording(rows as f64 / self.tick_rate_hz)
            }
            Phase::Saving(_) => FfbLogStatus::Saving,
            Phase::Done(status) => status.clone(),
        }
    }

    fn push_row(
        &mut self,
        session: &Session,
        frame: &InputFrame,
        input: &FfbInput,
        sample: &FfbSample,
    ) {
        let out = session.car().output();
        let t = session.timing();
        let (fl, fr) = (&out.wheels[0], &out.wheels[1]);
        let row: [f64; NCOL] = [
            session.ticks_to_seconds(session.tick()),
            session.ticks_to_seconds(session.tick() - t.start_tick),
            t.run_distance_um as f64 * 1.0e-6,
            out.speed,
            f64::from(frame.steer) / f64::from(AXIS_MAX),
            out.steering_wheel_angle,
            input.device_angle_rad.unwrap_or(f64::NAN),
            out.steering_torque_nm,
            session.impact_kick_nm(),
            sample.raw_nm,
            sample.scaled_nm,
            sample.filtered_nm,
            sample.effects_nm,
            sample.detail_nm,
            sample.requested,
            sample.output,
            sample.clipped,
            input.kerb_contact,
            input.front_load_difference_n,
            fl.slip_angle,
            fr.slip_angle,
            fl.fy,
            fr.fy,
            fl.mz,
            fr.mz,
            fl.load,
            fr.load,
            out.accel_body[1],
            out.accel_body[0],
        ];
        self.rows.extend_from_slice(&row);
    }

    /// Hands the rows to a writer thread, so the physics thread never
    /// waits on the disk.
    fn save(&mut self, session: &Session, config: &FfbConfig, counted: bool) {
        let header = header(session, config, counted);
        let rows = std::mem::take(&mut self.rows);
        let path = self.path.clone();
        let spawned = std::thread::Builder::new()
            .name("ffb-log".into())
            .spawn(move || write_csv(&path, &header, &rows).map(|()| (path, counted)));
        self.phase = match spawned {
            Ok(handle) => Phase::Saving(handle),
            Err(e) => Phase::Done(FfbLogStatus::Failed(format!(
                "could not start the writer: {e}"
            ))),
        };
    }
}

fn header(session: &Session, c: &FfbConfig, counted: bool) -> String {
    let mut h = String::new();
    let t = session.timing();
    let _ = writeln!(
        h,
        "# {PRODUCT_NAME} force-feedback log, core {CORE_VERSION}"
    );
    let _ = writeln!(h, "# car = {}", session.car().params().id);
    let _ = writeln!(h, "# track = {}", session.track().def().id);
    let _ = writeln!(h, "# tick_rate_hz = {}", session.tick_rate_hz());
    let _ = writeln!(h, "# lap_counted = {counted}");
    let _ = writeln!(
        h,
        "# lap_time_s = {:.3}",
        session.ticks_to_seconds(t.last_ticks)
    );
    let _ = writeln!(h, "# ffb_preset = {}", c.id);
    for (name, value) in [
        ("gain", c.gain),
        ("device_max_torque_nm", c.device_max_torque_nm),
        ("device_rotation_rad", c.device_rotation_rad),
        ("lowpass_cutoff_hz", c.lowpass_cutoff_hz),
        ("slew_limit_nm_per_s", c.slew_limit_nm_per_s),
        ("soft_clip_knee", c.soft_clip_knee),
        ("min_force", c.min_force),
        ("damping_nm_s_per_rad", c.damping_nm_s_per_rad),
        ("friction_nm", c.friction_nm),
        ("inertia_nm_s2_per_rad", c.inertia_nm_s2_per_rad),
        ("road_detail_gain_nm_per_n", c.road_detail_gain_nm_per_n),
        ("kerb_gain_nm_per_n", c.kerb_gain_nm_per_n),
        ("detail_limit_nm", c.detail_limit_nm),
    ] {
        let _ = writeln!(h, "# {name} = {value}");
    }
    let _ = writeln!(h, "# invert = {}", c.invert);
    for (name, meaning) in COLUMNS {
        let _ = writeln!(h, "# column {name}: {meaning}");
    }
    let names: Vec<&str> = COLUMNS.iter().map(|(n, _)| *n).collect();
    let _ = writeln!(h, "{}", names.join(","));
    h
}

/// Decimal places kept for a column: enough to resolve what the column
/// can show, and no more, so a lap stays a manageable file.
fn decimals(name: &str) -> usize {
    if name.ends_with("_rad") {
        5
    } else if name.ends_with("_n") {
        1
    } else if name.ends_with("_s")
        || matches!(name, "requested" | "output" | "clipped" | "steer_input")
    {
        4
    } else {
        3
    }
}

fn write_csv(path: &Path, header: &str, rows: &[f64]) -> Result<(), String> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut w = std::io::BufWriter::new(file);
    let mut line = String::new();
    let places: Vec<usize> = COLUMNS.iter().map(|(n, _)| decimals(n)).collect();
    let result = (|| -> std::io::Result<()> {
        w.write_all(header.as_bytes())?;
        for row in rows.chunks_exact(NCOL) {
            line.clear();
            for (i, v) in row.iter().enumerate() {
                if i > 0 {
                    line.push(',');
                }
                if v.is_finite() {
                    let _ = write!(line, "{v:.*}", places[i]);
                }
            }
            line.push('\n');
            w.write_all(line.as_bytes())?;
        }
        w.flush()
    })();
    result.map_err(|e| format!("{}: {e}", path.display()))
}

// ----- reading a log back --------------------------------------------------

/// A log read from a file.
#[derive(Clone, Debug, Default)]
pub struct FfbLogData {
    /// `key = value` lines from the comment header.
    pub meta: Vec<(String, String)>,
    /// Column names.
    pub columns: Vec<String>,
    /// Rows, each `columns.len()` long; empty cells are NaN.
    pub rows: Vec<Vec<f64>>,
}

impl FfbLogData {
    /// Parses a log.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut data = FfbLogData::default();
        for (n, line) in text.lines().enumerate() {
            if let Some(comment) = line.strip_prefix('#') {
                if let Some((k, v)) = comment.split_once('=') {
                    if !k.trim().starts_with("column ") {
                        data.meta.push((k.trim().to_string(), v.trim().to_string()));
                    }
                }
                continue;
            }
            if line.trim().is_empty() {
                continue;
            }
            if data.columns.is_empty() {
                data.columns = line.split(',').map(|s| s.trim().to_string()).collect();
                continue;
            }
            let row: Vec<f64> = line
                .split(',')
                .map(|s| s.trim().parse::<f64>().unwrap_or(f64::NAN))
                .collect();
            if row.len() != data.columns.len() {
                return Err(format!(
                    "line {}: expected {} values",
                    n + 1,
                    data.columns.len()
                ));
            }
            data.rows.push(row);
        }
        if data.columns.is_empty() || data.rows.is_empty() {
            return Err("no data in the log".into());
        }
        Ok(data)
    }

    /// A header value.
    pub fn meta(&self, key: &str) -> Option<&str> {
        self.meta
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// A column by name.
    pub fn column(&self, name: &str) -> Result<Vec<f64>, String> {
        let i = self
            .columns
            .iter()
            .position(|c| c == name)
            .ok_or_else(|| format!("the log has no `{name}` column"))?;
        Ok(self.rows.iter().map(|r| r[i]).collect())
    }
}

/// A stretch of a lap where the output was clipping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipStretch {
    /// Distance into the lap where it began, m.
    pub from_m: f64,
    /// Where it ended, m.
    pub to_m: f64,
    /// Duration, s.
    pub seconds: f64,
    /// Mean speed, m/s.
    pub speed_m_s: f64,
    /// Largest `|requested|` in it.
    pub peak_requested: f64,
    /// Largest lateral acceleration in it, m/s².
    pub peak_lat_g: f64,
}

/// Mean torque in one band of front slip angle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlipBand {
    /// Lower edge of the band, degrees.
    pub from_deg: f64,
    /// Ticks in the band.
    pub ticks: usize,
    /// Mean `|column_torque_nm|`.
    pub column_nm: f64,
    /// Mean `|output|`.
    pub output: f64,
}

/// Figures from one log.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FfbLogSummary {
    /// Seconds of log.
    pub seconds: f64,
    /// Share of ticks clipping.
    pub clip_share: f64,
    /// Largest `|requested|`.
    pub peak_requested: f64,
    /// 99th percentile of `|requested|`.
    pub p99_requested: f64,
    /// Largest `|column_torque_nm|`.
    pub peak_column_nm: f64,
    /// Clipping stretches, longest first.
    pub stretches: Vec<ClipStretch>,
    /// Torque against front slip angle, 1° bands from zero, while the car
    /// is cornering above walking pace and off the kerbs.
    pub slip_bands: Vec<SlipBand>,
}

fn percentile(values: &mut [f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let i = ((values.len() - 1) as f64 * p).round() as usize;
    values[i]
}

/// Works out the figures for one log.
pub fn summarize(data: &FfbLogData) -> Result<FfbLogSummary, String> {
    let time = data.column("run_time_s")?;
    let dist = data.column("distance_m")?;
    let speed = data.column("speed_m_s")?;
    let requested = data.column("requested")?;
    let clipped = data.column("clipped")?;
    let output = data.column("output")?;
    let column = data.column("column_torque_nm")?;
    let lat = data.column("accel_lat_m_s2")?;
    let kerb = data.column("kerb_contact")?;
    let fl = data.column("fl_slip_angle_rad")?;
    let fr = data.column("fr_slip_angle_rad")?;
    let n = time.len();
    let dt = if n > 1 {
        (time[n - 1] - time[0]) / (n - 1) as f64
    } else {
        0.001
    };
    let clipping: Vec<bool> = clipped.iter().map(|c| *c > CLIP_THRESHOLD).collect();
    let mut s = FfbLogSummary {
        seconds: n as f64 * dt,
        clip_share: clipping.iter().filter(|c| **c).count() as f64 / n as f64,
        peak_requested: requested.iter().fold(0.0_f64, |m, r| m.max(r.abs())),
        p99_requested: percentile(
            &mut requested.iter().map(|r| r.abs()).collect::<Vec<_>>(),
            0.99,
        ),
        peak_column_nm: column.iter().fold(0.0_f64, |m, r| m.max(r.abs())),
        ..FfbLogSummary::default()
    };

    // Clipping stretches, joining gaps shorter than 0.2 s.
    let gap = (0.2 / dt.max(1e-6)) as usize;
    let mut i = 0;
    while i < n {
        if !clipping[i] {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i;
        let mut j = i;
        while j < n && j <= end + gap {
            if clipping[j] {
                end = j;
            }
            j += 1;
        }
        let range = start..=end;
        let count = (end - start + 1) as f64;
        s.stretches.push(ClipStretch {
            from_m: dist[start],
            to_m: dist[end],
            seconds: count * dt,
            speed_m_s: range.clone().map(|k| speed[k]).sum::<f64>() / count,
            peak_requested: range
                .clone()
                .fold(0.0_f64, |m, k| m.max(requested[k].abs())),
            peak_lat_g: range.fold(0.0_f64, |m, k| m.max(lat[k].abs())) / 9.81,
        });
        i = end + 1;
    }
    s.stretches.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));

    // Torque against front slip angle.
    let mut bands: Vec<(usize, f64, f64)> = vec![(0, 0.0, 0.0); 16];
    for k in 0..n {
        if speed[k] < 8.0 || kerb[k] > 0.0 {
            continue;
        }
        let alpha = 0.5 * (fl[k].abs() + fr[k].abs());
        let band = (alpha.to_degrees()).floor() as usize;
        if let Some(b) = bands.get_mut(band) {
            b.0 += 1;
            b.1 += column[k].abs();
            b.2 += output[k].abs();
        }
    }
    s.slip_bands = bands
        .iter()
        .enumerate()
        .filter(|(_, b)| b.0 > 0)
        .map(|(i, b)| SlipBand {
            from_deg: i as f64,
            ticks: b.0,
            column_nm: b.1 / b.0 as f64,
            output: b.2 / b.0 as f64,
        })
        .collect();
    Ok(s)
}

/// A readable report on one log, with what to change in the preset.
pub fn report(text: &str) -> Result<String, String> {
    let data = FfbLogData::parse(text)?;
    let s = summarize(&data)?;
    let mut r = String::new();
    let meta = |k: &str| data.meta(k).unwrap_or("?").to_string();
    let _ = writeln!(
        r,
        "{} on {}, preset {}, lap {} s (counted: {})",
        meta("car"),
        meta("track"),
        meta("ffb_preset"),
        meta("lap_time_s"),
        meta("lap_counted")
    );
    let _ = writeln!(
        r,
        "gain {}, peak {} N.m, knee {}, min force {}",
        meta("gain"),
        meta("device_max_torque_nm"),
        meta("soft_clip_knee"),
        meta("min_force")
    );
    let _ = writeln!(
        r,
        "{:.1} s logged; clipping {:.1}% of the lap; requested peak {:.2}, 99th percentile {:.2}; column torque peak {:.1} N.m",
        s.seconds,
        s.clip_share * 100.0,
        s.peak_requested,
        s.p99_requested,
        s.peak_column_nm
    );
    if s.stretches.is_empty() {
        let _ = writeln!(r, "No clipping.");
    } else {
        let _ = writeln!(r, "Longest clipping stretches (lap distance):");
        for c in s.stretches.iter().take(8) {
            let _ = writeln!(
                r,
                "  {:6.0}-{:6.0} m  {:5.2} s  {:5.1} km/h  requested {:4.2}  {:4.2} g",
                c.from_m,
                c.to_m,
                c.seconds,
                c.speed_m_s * 3.6,
                c.peak_requested,
                c.peak_lat_g
            );
        }
    }
    let _ = writeln!(
        r,
        "Column torque and output against front slip angle (off kerbs, above 29 km/h):"
    );
    let peak_band = s
        .slip_bands
        .iter()
        .filter(|b| b.ticks >= 50)
        .max_by(|a, b| a.column_nm.total_cmp(&b.column_nm));
    for b in &s.slip_bands {
        let mark = if peak_band.is_some_and(|p| p.from_deg == b.from_deg) {
            "  <- torque peak"
        } else {
            ""
        };
        let _ = writeln!(
            r,
            "  {:2.0}-{:2.0} deg  {:6} ticks  column {:5.1} N.m  output {:4.2}{mark}",
            b.from_deg,
            b.from_deg + 1.0,
            b.ticks,
            b.column_nm,
            b.output
        );
    }
    if let Some(p) = peak_band {
        let beyond: Vec<&SlipBand> = s
            .slip_bands
            .iter()
            .filter(|b| b.from_deg > p.from_deg && b.ticks >= 50)
            .collect();
        if let Some(last) = beyond.last() {
            let column_drop = 1.0 - last.column_nm / p.column_nm;
            let output_drop = 1.0 - last.output / p.output.max(1e-9);
            let _ = writeln!(
                r,
                "Past the peak the column torque falls {:.0}% and the output {:.0}%.{}",
                column_drop * 100.0,
                output_drop * 100.0,
                if output_drop < 0.5 * column_drop {
                    " The output hides most of the drop: clipping is flattening the grip limit."
                } else {
                    ""
                }
            );
        } else {
            let _ = writeln!(
                r,
                "The front tires were not driven past their torque peak in this lap."
            );
        }
    }
    let knee: f64 = meta("soft_clip_knee").parse().unwrap_or(0.0);
    let gain: f64 = meta("gain").parse().unwrap_or(0.0);
    if knee > 0.0 && gain > 0.0 && s.p99_requested > 0.0 {
        let _ = writeln!(
            r,
            "Gain that puts the 99th percentile at the knee: {:.3} (now {gain}).",
            gain * knee / s.p99_requested
        );
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log_text(rows: &[[f64; NCOL]]) -> String {
        let mut t = String::from("# car = test\n# gain = 0.3\n# soft_clip_knee = 0.75\n");
        let names: Vec<&str> = COLUMNS.iter().map(|(n, _)| *n).collect();
        t.push_str(&names.join(","));
        t.push('\n');
        for r in rows {
            let cells: Vec<String> = r.iter().map(|v| format!("{v}")).collect();
            t.push_str(&cells.join(","));
            t.push('\n');
        }
        t
    }

    fn col(name: &str) -> usize {
        COLUMNS.iter().position(|(n, _)| *n == name).unwrap()
    }

    #[test]
    fn summary_finds_clipping_and_the_torque_peak() {
        let mut rows = Vec::new();
        for i in 0..4000 {
            let mut r = [0.0; NCOL];
            let t = i as f64 * 0.001;
            r[col("run_time_s")] = t;
            r[col("distance_m")] = t * 30.0;
            r[col("speed_m_s")] = 30.0;
            // Slip angle sweeps 0 to 8 degrees; torque peaks at 4.
            let alpha = (8.0 * t / 4.0).to_radians();
            r[col("fl_slip_angle_rad")] = alpha;
            r[col("fr_slip_angle_rad")] = alpha;
            let deg = alpha.to_degrees();
            let torque = if deg < 4.0 {
                4.0 * deg
            } else {
                16.0 - 2.0 * (deg - 4.0)
            };
            r[col("column_torque_nm")] = torque;
            r[col("requested")] = torque * 0.3 / 6.0;
            r[col("output")] = r[col("requested")].min(0.75);
            r[col("clipped")] = (r[col("requested")] - 0.75).max(0.0);
            rows.push(r);
        }
        let data = FfbLogData::parse(&log_text(&rows)).unwrap();
        let s = summarize(&data).unwrap();
        assert!((s.peak_column_nm - 16.0).abs() < 0.1);
        // Clipping only near the torque peak, where 0.8 is asked for.
        assert!(s.clip_share > 0.0 && s.clip_share < 0.5, "{}", s.clip_share);
        let peak = s
            .slip_bands
            .iter()
            .max_by(|a, b| a.column_nm.total_cmp(&b.column_nm))
            .unwrap();
        assert!(peak.from_deg == 3.0 || peak.from_deg == 4.0);
        let text = report(&log_text(&rows)).unwrap();
        assert!(text.contains("torque peak"), "{text}");
        assert!(text.contains("Past the peak"), "{text}");
    }

    #[test]
    fn clipping_stretches_join_short_gaps() {
        let mut rows = Vec::new();
        for i in 0..3000 {
            let mut r = [0.0; NCOL];
            r[col("run_time_s")] = i as f64 * 0.001;
            r[col("distance_m")] = i as f64 * 0.03;
            r[col("speed_m_s")] = 30.0;
            let on =
                (500..900).contains(&i) || (950..1200).contains(&i) || (2500..2600).contains(&i);
            r[col("requested")] = if on { 1.4 } else { 0.5 };
            r[col("clipped")] = if on { 0.5 } else { 0.0 };
            rows.push(r);
        }
        let s = summarize(&FfbLogData::parse(&log_text(&rows)).unwrap()).unwrap();
        assert_eq!(s.stretches.len(), 2);
        assert!((s.stretches[0].seconds - 0.7).abs() < 0.002);
        assert!((s.clip_share - 0.75 / 3.0).abs() < 1e-9);
        assert!((s.peak_requested - 1.4).abs() < 1e-12);
    }

    #[test]
    fn a_short_or_malformed_log_is_refused() {
        assert!(FfbLogData::parse("").is_err());
        assert!(FfbLogData::parse("a,b\n1\n").is_err());
    }
}
