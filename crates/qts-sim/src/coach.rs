// SPDX-License-Identifier: Apache-2.0
//! "Why am I slow": splits a lap into corners and says where, against a
//! reference lap, time was lost and why. It reads per-lap telemetry only
//! ([`Lap`]: time, speed, throttle and yaw rate by distance) and
//! changes nothing in the simulation.
//!
//! Corners are found on the reference lap, where the path it drove curves
//! tighter than [`CORNER_RADIUS_M`]. The lap is cut into one sector per
//! corner, with the cuts halfway along the straights. Each sector is
//! split again at the reference's braking point, its slowest point and its
//! return to throttle. The phase where most time went, together with what
//! the driver did differently there, names the cause:
//!
//! - earlier braking: began slowing for the corner [`MIN_POINT_DIFF_M`]
//!   or more before the reference, and lost the time in the run to it.
//!   The braking point is where the car stops gaining speed on the
//!   approach and starts to slow: the brake going on, or a lift where a
//!   lift is enough;
//! - later braking: braked that much after the reference and lost time
//!   through the braking zone and the slowest part (an overshoot);
//! - lower minimum speed: slowest point at least [`MIN_SPEED_DIFF_KMH`]
//!   slower, time lost from braking to apex;
//! - later throttle: back on the power that much later, time lost on the
//!   way out.

use crate::lap_telemetry::Lap;
use qts_core::step::Simulation;

/// Paths tighter than this radius are a corner, m.
pub const CORNER_RADIUS_M: f64 = 120.0;
/// Shortest stretch of curve counted as a corner, m.
pub const MIN_CORNER_LENGTH_M: f64 = 12.0;
/// Corners closer than this are one corner (an S-bend, a double apex), m.
pub const MERGE_GAP_M: f64 = 20.0;
/// Curvature is smoothed over this distance either side, m.
pub const SMOOTH_M: f64 = 5.0;
/// Braking or throttle points closer than this are "the same", m.
pub const MIN_POINT_DIFF_M: f64 = 3.0;
/// Minimum speeds closer than this are "the same", km/h.
pub const MIN_SPEED_DIFF_KMH: f64 = 1.0;
/// A corner the car slows less than this for, km/h, has no braking
/// point: it is taken flat.
pub const MIN_SLOWING_KMH: f64 = 2.0;
/// Throttle above this is back on the power.
pub const THROTTLE_ON: f64 = 0.5;

/// One corner of the reference lap, by distance along it, m.
#[derive(Clone, Debug, PartialEq)]
pub struct Corner {
    /// 1 for the first corner after the line.
    pub number: usize,
    /// True when it turns left.
    pub left: bool,
    /// The sector given to this corner: from halfway along the straight
    /// before it to halfway along the one after.
    pub sector: (f64, f64),
    /// The curved part.
    pub curve: (f64, f64),
}

/// Why a corner was slower than the reference.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cause {
    /// Braked this many metres before the reference.
    EarlierBraking(f64),
    /// Braked this many metres after the reference.
    LaterBraking(f64),
    /// Slowest point this many km/h slower than the reference's.
    LowerMinimumSpeed(f64),
    /// Back on the throttle this many metres after the reference.
    LaterThrottle(f64),
    /// Time lost, but none of the above stands out.
    Unclear,
    /// No time lost.
    None,
}

impl Cause {
    /// Short description for the screen.
    pub fn describe(&self) -> String {
        match *self {
            Cause::EarlierBraking(m) => format!("braked or lifted {m:.0} m earlier"),
            Cause::LaterBraking(m) => format!("braked {m:.0} m later and overshot"),
            Cause::LowerMinimumSpeed(k) => format!("minimum speed {k:.1} km/h lower"),
            Cause::LaterThrottle(m) => format!("back on the throttle {m:.0} m later"),
            Cause::Unclear => "no single cause stands out".into(),
            Cause::None => "no time lost".into(),
        }
    }

    /// Stable identifier for the front end.
    pub fn key(&self) -> &'static str {
        match self {
            Cause::EarlierBraking(_) => "earlier_braking",
            Cause::LaterBraking(_) => "later_braking",
            Cause::LowerMinimumSpeed(_) => "lower_min_speed",
            Cause::LaterThrottle(_) => "later_throttle",
            Cause::Unclear => "unclear",
            Cause::None => "none",
        }
    }
}

/// What one lap did in one corner, against the reference.
#[derive(Clone, Debug, PartialEq)]
pub struct CornerReport {
    /// The corner.
    pub corner: Corner,
    /// Time lost in the corner's sector, s (negative: gained).
    pub time_lost_s: f64,
    /// Time lost in each phase: to the reference's braking point, to its
    /// slowest point, to its return to throttle, and to the sector's end.
    pub phase_lost_s: [f64; 4],
    /// The cause given.
    pub cause: Cause,
    /// Braking point (where the car starts slowing), m along the
    /// reference: this lap, reference. `None` when the corner was taken
    /// without slowing.
    pub brake_m: (Option<f64>, Option<f64>),
    /// Slowest speed in the corner, km/h: this lap, reference.
    pub min_speed_kmh: (f64, f64),
    /// Return to throttle after the slowest point, m: this lap, reference.
    pub throttle_m: (Option<f64>, Option<f64>),
}

/// A lap against a reference, corner by corner.
#[derive(Clone, Debug, PartialEq)]
pub struct CoachReport {
    /// Lap time, s.
    pub lap_s: f64,
    /// Reference lap time, s.
    pub reference_s: f64,
    /// One entry per corner, in order. The sectors cover the whole lap, so
    /// the losses add up to the difference in lap time.
    pub corners: Vec<CornerReport>,
}

impl CoachReport {
    /// The corners where most time went, largest first, at most `n`.
    pub fn worst(&self, n: usize) -> Vec<&CornerReport> {
        let mut v: Vec<&CornerReport> = self
            .corners
            .iter()
            .filter(|c| c.time_lost_s > 0.0)
            .collect();
        v.sort_by(|a, b| b.time_lost_s.total_cmp(&a.time_lost_s));
        v.truncate(n);
        v
    }
}

/// The columns the coach reads, by distance.
struct Trace<'a> {
    lap: &'a Lap,
    speed: Vec<f64>,
    throttle: Vec<f64>,
    yaw_rate: Option<Vec<f64>>,
    /// Factor from the reference's distance to this lap's.
    scale: f64,
}

impl<'a> Trace<'a> {
    fn new(lap: &'a Lap, reference_length: f64) -> Result<Self, String> {
        let col = |name: &str| {
            lap.file
                .column(name)
                .ok_or_else(|| format!("{}: no {name} column", lap.label))
        };
        let length = lap.length();
        if length <= 0.0 {
            return Err(format!("{}: no distance covered", lap.label));
        }
        Ok(Self {
            lap,
            speed: col("speed_m_s")?,
            throttle: col("throttle")?,
            yaw_rate: lap.file.column("yaw_rate_rad_s"),
            scale: length / reference_length,
        })
    }

    /// A column at a distance along the reference.
    fn at(&self, column: &[f64], d_ref: f64) -> f64 {
        self.lap.at(column, d_ref * self.scale)
    }

    fn time(&self, d_ref: f64) -> f64 {
        self.at(&self.lap.time, d_ref)
    }

    /// Rows with their distance along the reference, between two
    /// reference distances.
    fn rows(&self, from: f64, to: f64) -> impl Iterator<Item = (usize, f64)> + '_ {
        self.lap
            .distance
            .iter()
            .enumerate()
            .map(move |(i, d)| (i, d / self.scale))
            .filter(move |&(_, d)| d >= from && d <= to)
    }

    /// Slowest point within a stretch: (distance, speed).
    fn slowest(&self, from: f64, to: f64) -> (f64, f64) {
        self.rows(from, to)
            .map(|(i, d)| (d, self.speed[i]))
            .fold((from, f64::MAX), |b, x| if x.1 < b.1 { x } else { b })
    }

    /// Where the car starts slowing for a corner whose slowest point is
    /// at `apex`: the fastest point of the approach from `from`. `None`
    /// when it slows by less than [`MIN_SLOWING_KMH`].
    fn brake_point(&self, from: f64, apex: f64) -> Option<f64> {
        let (d, v) = self
            .rows(from, apex)
            .map(|(i, d)| (d, self.speed[i]))
            .fold((from, f64::MIN), |b, x| if x.1 >= b.1 { x } else { b });
        let at_apex = self.at(&self.speed, apex);
        ((v - at_apex) * 3.6 >= MIN_SLOWING_KMH).then_some(d)
    }

    /// First return to throttle after `after`, before `to`.
    fn throttle_point(&self, after: f64, to: f64) -> Option<f64> {
        self.rows(after, to)
            .find(|&(i, _)| self.throttle[i] > THROTTLE_ON)
            .map(|(_, d)| d)
    }
}

/// Finds the corners of a lap from the curvature of the path it drove
/// (yaw rate over speed).
pub fn find_corners(reference: &Lap) -> Result<Vec<Corner>, String> {
    let t = Trace::new(reference, reference.length())?;
    let yaw = t
        .yaw_rate
        .as_ref()
        .ok_or_else(|| format!("{}: no yaw_rate_rad_s column", reference.label))?;
    let d = &reference.distance;
    let n = d.len();
    let raw: Vec<f64> = (0..n).map(|i| yaw[i] / t.speed[i].max(3.0)).collect();
    // Smooth over SMOOTH_M either side.
    let mut curv = vec![0.0; n];
    let (mut lo, mut hi, mut sum) = (0usize, 0usize, 0.0);
    for i in 0..n {
        while hi < n && d[hi] <= d[i] + SMOOTH_M {
            sum += raw[hi];
            hi += 1;
        }
        while d[lo] < d[i] - SMOOTH_M {
            sum -= raw[lo];
            lo += 1;
        }
        curv[i] = sum / (hi - lo).max(1) as f64;
    }
    // Stretches tighter than the corner radius.
    let mut spans: Vec<(f64, f64, f64)> = Vec::new(); // start, end, signed curvature sum
    let mut open: Option<(f64, f64)> = None;
    for i in 0..n {
        let tight = curv[i].abs() > 1.0 / CORNER_RADIUS_M;
        match (&mut open, tight) {
            (None, true) => open = Some((d[i], curv[i])),
            (Some((_, s)), true) => *s += curv[i],
            (Some((start, s)), false) => {
                spans.push((*start, d[i], *s));
                open = None;
            }
            (None, false) => {}
        }
    }
    if let Some((start, s)) = open {
        spans.push((start, d[n - 1], s));
    }
    // Merge close ones, then drop short ones.
    let mut merged: Vec<(f64, f64, f64)> = Vec::new();
    for s in spans {
        match merged.last_mut() {
            Some(last) if s.0 - last.1 < MERGE_GAP_M => {
                last.1 = s.1;
                last.2 += s.2;
            }
            _ => merged.push(s),
        }
    }
    merged.retain(|s| s.1 - s.0 >= MIN_CORNER_LENGTH_M);
    let length = reference.length();
    let count = merged.len();
    Ok(merged
        .iter()
        .enumerate()
        .map(|(k, s)| {
            let start = if k == 0 {
                0.0
            } else {
                0.5 * (merged[k - 1].1 + s.0)
            };
            let end = if k + 1 == count {
                length
            } else {
                0.5 * (s.1 + merged[k + 1].0)
            };
            Corner {
                number: k + 1,
                left: s.2 > 0.0,
                sector: (start, end),
                curve: (s.0, s.1),
            }
        })
        .collect())
}

/// Compares `lap` with `reference`, corner by corner.
pub fn analyse(lap: &Lap, reference: &Lap) -> Result<CoachReport, String> {
    let corners = find_corners(reference)?;
    let r = Trace::new(reference, reference.length())?;
    let l = Trace::new(lap, reference.length())?;
    let lost = |d: f64| l.time(d) - r.time(d);
    let mut out = Vec::with_capacity(corners.len());
    for c in corners {
        let (s0, s1) = c.sector;
        // The reference's markers.
        let (r_apex, r_min) = r.slowest(c.curve.0, c.curve.1);
        let r_brake = r.brake_point(s0, r_apex);
        let r_throttle = r.throttle_point(r_apex, s1);
        // This lap's own.
        let (l_apex, l_min) = l.slowest(c.curve.0, c.curve.1);
        let l_brake = l.brake_point(s0, l_apex);
        let l_throttle = l.throttle_point(l_apex, s1);
        // Phase boundaries on the reference; a missing marker collapses
        // its phase onto the next.
        let b1 = r_brake.unwrap_or(r_apex).clamp(s0, s1);
        let b2 = r_apex.clamp(b1, s1);
        let b3 = r_throttle.unwrap_or(b2).clamp(b2, s1);
        let marks = [s0, b1, b2, b3, s1];
        let mut phase = [0.0; 4];
        for k in 0..4 {
            phase[k] = lost(marks[k + 1]) - lost(marks[k]);
        }
        let total = lost(s1) - lost(s0);
        // Candidate causes with the time they account for.
        let mut candidates: Vec<(Cause, f64)> = Vec::new();
        if let (Some(lb), Some(rb)) = (l_brake, r_brake) {
            if rb - lb >= MIN_POINT_DIFF_M {
                candidates.push((Cause::EarlierBraking(rb - lb), phase[0] + phase[1]));
            } else if lb - rb >= MIN_POINT_DIFF_M {
                candidates.push((Cause::LaterBraking(lb - rb), phase[1] + phase[2]));
            }
        } else if let (Some(lb), None) = (l_brake, r_brake) {
            // Braked where the reference did not.
            candidates.push((
                Cause::EarlierBraking((b1 - lb).max(0.0)),
                phase[0] + phase[1],
            ));
        }
        let speed_diff = (r_min - l_min) * 3.6;
        if speed_diff >= MIN_SPEED_DIFF_KMH {
            candidates.push((Cause::LowerMinimumSpeed(speed_diff), phase[1] + phase[2]));
        }
        match (l_throttle, r_throttle) {
            (Some(lt), Some(rt)) if lt - rt >= MIN_POINT_DIFF_M => {
                candidates.push((Cause::LaterThrottle(lt - rt), phase[3]));
            }
            (None, Some(rt)) => candidates.push((Cause::LaterThrottle(s1 - rt), phase[3])),
            _ => {}
        }
        let cause = if total <= 0.0 {
            Cause::None
        } else {
            candidates
                .into_iter()
                .filter(|(_, t)| *t > 0.0)
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map_or(Cause::Unclear, |(c, _)| c)
        };
        out.push(CornerReport {
            corner: c,
            time_lost_s: total,
            phase_lost_s: phase,
            cause,
            brake_m: (l_brake, r_brake),
            min_speed_kmh: (l_min * 3.6, r_min * 3.6),
            throttle_m: (l_throttle, r_throttle),
        });
    }
    Ok(CoachReport {
        lap_s: *lap.time.last().unwrap_or(&0.0),
        reference_s: *reference.time.last().unwrap_or(&0.0),
        corners: out,
    })
}

/// The report as text, one line per corner.
pub fn report_text(report: &CoachReport) -> String {
    let mut s = format!(
        "lap {:.3} s against {:.3} s: {:+.3} s\n",
        report.lap_s,
        report.reference_s,
        report.lap_s - report.reference_s
    );
    for c in &report.corners {
        s.push_str(&format!(
            "T{:<2} {:<5} {:+.3} s  {}\n",
            c.corner.number,
            if c.corner.left { "left" } else { "right" },
            c.time_lost_s,
            if c.time_lost_s > 0.0 {
                c.cause.describe()
            } else {
                "gained".into()
            }
        ));
    }
    s
}

/// Columns of the trace the live coach keeps, in this order.
pub const TRACE_COLUMNS: [&str; 5] = ["time_s", "speed_m_s", "throttle", "brake", "yaw_rate_rad_s"];

/// The live coach's state shared with the front end.
#[derive(Clone, Debug, Default)]
pub struct CoachSlot {
    /// Counts the reports made, so the front end can tell a new one.
    pub serial: u32,
    /// The newest report, against the best lap before it.
    pub report: Option<std::sync::Arc<CoachReport>>,
    /// Problems writing or reading the reference file.
    pub error: Option<String>,
}

/// Keeps a light trace of the lap in progress (the [`TRACE_COLUMNS`] at
/// about 100 rows a second) and, when a lap counts, compares it with the
/// best lap so far on a background thread. A faster lap becomes the new
/// reference and, given a file, is saved there in the telemetry format so
/// it survives the session. Like the lap recorder it only reads the
/// session.
pub struct CoachRecorder {
    decimation: u32,
    counter: u32,
    seen: Option<(u32, u32)>,
    rows: Vec<Vec<f64>>,
    metadata: Vec<(String, String)>,
    reference: Option<std::sync::Arc<Lap>>,
    file: Option<std::path::PathBuf>,
    slot: std::sync::Arc<std::sync::Mutex<CoachSlot>>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

impl CoachRecorder {
    /// A recorder for `session`. `file`, if given, holds the reference lap:
    /// it is read now if it was made for this car, track and content, and
    /// rewritten on every faster lap.
    pub fn new(session: &crate::session::Session, file: Option<std::path::PathBuf>) -> Self {
        let rate = f64::from(session.tick_rate_hz());
        let decimation = (rate / 100.0).round().max(1.0) as u32;
        let metadata = crate::telemetry::metadata(session, rate / f64::from(decimation));
        let reference = file
            .as_ref()
            .and_then(|f| Lap::read(f).ok())
            .filter(|lap| {
                ["car", "track", "content_hash"].iter().all(|k| {
                    lap.file.meta(k)
                        == metadata
                            .iter()
                            .find(|(m, _)| m == k)
                            .map(|(_, v)| v.as_str())
                })
            })
            .map(std::sync::Arc::new);
        Self {
            decimation,
            counter: 0,
            seen: None,
            rows: Vec::new(),
            metadata,
            reference,
            file,
            slot: std::sync::Arc::default(),
            workers: Vec::new(),
        }
    }

    /// The shared state the front end reads.
    pub fn slot(&self) -> std::sync::Arc<std::sync::Mutex<CoachSlot>> {
        std::sync::Arc::clone(&self.slot)
    }

    /// True when there is a best lap to compare with.
    pub fn has_reference(&self) -> bool {
        self.reference.is_some()
    }

    /// Call after every tick with the frame the tick used.
    pub fn observe(
        &mut self,
        session: &crate::session::Session,
        frame: &qts_core::input::InputFrame,
    ) {
        let t = session.timing();
        let finished = (t.runs, t.invalid_runs);
        if let Some(before) = self.seen {
            if finished != before && !self.rows.is_empty() {
                let rows = std::mem::take(&mut self.rows);
                if finished.0 > before.0 {
                    self.lap_done(rows);
                }
            }
        }
        self.seen = Some(finished);
        if !t.running {
            self.rows.clear();
            self.counter = 0;
            return;
        }
        if self.counter == 0 {
            let o = session.car().output();
            let time = session.tick().saturating_sub(t.start_tick) as f64
                / f64::from(session.tick_rate_hz());
            self.rows.push(vec![
                time,
                o.speed,
                frame.throttle_f64(),
                frame.brake_f64(),
                o.yaw_rate,
            ]);
        }
        self.counter = (self.counter + 1) % self.decimation;
    }

    fn lap_done(&mut self, rows: Vec<Vec<f64>>) {
        let file = qts_telemetry::TelemetryFile {
            metadata: self.metadata.clone(),
            columns: TRACE_COLUMNS.iter().map(|c| c.to_string()).collect(),
            rows,
        };
        let Ok(lap) = Lap::from_file("this lap".into(), file) else {
            return;
        };
        let lap = std::sync::Arc::new(lap);
        let reference = self.reference.clone();
        let faster = reference
            .as_ref()
            .is_none_or(|r| lap.time.last() < r.time.last());
        if faster {
            self.reference = Some(std::sync::Arc::clone(&lap));
        }
        let slot = std::sync::Arc::clone(&self.slot);
        let save_to = if faster { self.file.clone() } else { None };
        self.workers.retain(|w| !w.is_finished());
        self.workers.push(std::thread::spawn(move || {
            let mut error = None;
            if let Some(path) = save_to {
                if let Err(e) = write_trace(&path, &lap) {
                    error = Some(format!("coach reference {}: {e}", path.display()));
                }
            }
            let report = reference.and_then(|r| analyse(&lap, &r).ok());
            let mut s = slot.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(report) = report {
                s.serial += 1;
                s.report = Some(std::sync::Arc::new(report));
            }
            if error.is_some() {
                s.error = error;
            }
        }));
    }

    /// Waits for reports and files in progress.
    pub fn wait(&mut self) {
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn write_trace(path: &std::path::Path, lap: &Lap) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    // In the order of the rows, which is not the full telemetry's.
    let all = crate::telemetry::channels();
    let channels: Vec<qts_telemetry::Channel> = TRACE_COLUMNS
        .iter()
        .filter_map(|n| all.iter().find(|c| c.name == *n).cloned())
        .collect();
    if channels.len() != TRACE_COLUMNS.len() {
        return Err("trace channel missing from the telemetry table".into());
    }
    let mut w = qts_telemetry::TelemetryWriter::new(
        std::io::BufWriter::new(f),
        &crate::telemetry::generator(),
        &lap.file.metadata,
        &channels,
        1,
    )
    .map_err(|e| e.to_string())?;
    for r in &lap.file.rows {
        w.sample(r).map_err(|e| e.to_string())?;
    }
    w.finish().map_err(|e| e.to_string())?;
    Ok(())
}
