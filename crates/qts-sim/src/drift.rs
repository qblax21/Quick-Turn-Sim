// SPDX-License-Identifier: Apache-2.0
//! Drift judging (D-114): each judged corner of a drift track is scored as
//! formal drift judging scores a run, on line, angle and style, from what
//! the car did through it. It only reads the session.
//!
//! - **Line** (35 points): how close the car comes to the inner clipping
//!   point (ICP), and how far its tail reaches into the outer clipping zone
//!   (OCZ), half each.
//! - **Angle** (35 points): the car's mean body slip angle through the
//!   zone, nothing at 5 degrees or less and full marks at 35 or more.
//! - **Style** (30 points): the share of the zone spent sliding (10
//!   degrees or more), less 30 percent of the marks for every time the car
//!   straightens and slides again within it.
//! - **Penalties**, which zero the corner: a spin (more than 110 degrees of
//!   slip), or all four wheels off the course for more than half a second.
//!
//! A lap's score is the mean of its corners' scores; a corner the car did
//! not pass through scores nothing.

use crate::session::Session;
use crate::track::{DriftDef, DriftZoneDef, Track};
use qts_core::math::Vec3;
use qts_core::step::Simulation;
use std::sync::{Arc, Mutex};

/// Weights of line, angle and style in a corner's score, out of 100.
pub const WEIGHTS: [f64; 3] = [35.0, 35.0, 30.0];
/// Slip angle that counts as sliding, degrees.
pub const SLIDE_DEG: f64 = 10.0;
/// Below this the car is taken to have straightened, degrees.
pub const STRAIGHT_DEG: f64 = 5.0;
/// Mean slip angle for no angle marks, and for full marks, degrees.
pub const ANGLE_RANGE_DEG: (f64, f64) = (5.0, 35.0);
/// Slip angle that is a spin, degrees.
pub const SPIN_DEG: f64 = 110.0;
/// Gap to the inner clipping point for full marks, and for none, m.
pub const ICP_RANGE_M: (f64, f64) = (0.5, 4.0);
/// How far short of the outer clipping zone's line the tail may stay and
/// still score anything, m.
pub const OCZ_SHORT_M: f64 = 4.0;
/// Time off the course that zeroes a corner, s.
pub const OFF_COURSE_S: f64 = 0.5;
/// Below this speed slip angle means nothing, m/s.
pub const MIN_SPEED: f64 = 5.0;
/// Rows a second the judge keeps.
pub const SAMPLE_HZ: f64 = 100.0;

/// What the car was doing at one moment in a zone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Distance along the path, m.
    pub s: f64,
    /// Body slip angle, degrees, unsigned (0 below [`MIN_SPEED`]).
    pub slip_deg: f64,
    /// Distance from the nearest corner of the car's body to the inner
    /// clipping point, m.
    pub icp_gap_m: f64,
    /// Offset of the middle of the rear bumper across the path, m,
    /// positive to the left.
    pub tail_offset_m: f64,
    /// True unless all four wheels are off the course.
    pub on_course: bool,
}

/// Why a corner scored nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Penalty {
    /// More than [`SPIN_DEG`] of slip.
    Spin,
    /// All four wheels off for more than [`OFF_COURSE_S`].
    OffCourse,
}

/// One corner's result.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneScore {
    /// The zone's name.
    pub name: String,
    /// Score out of 100.
    pub total: f64,
    /// Line, angle and style, each 0 to 1.
    pub line: f64,
    /// See `line`.
    pub angle: f64,
    /// See `line`.
    pub style: f64,
    /// Closest the car came to the inner clipping point, m.
    pub icp_gap_m: f64,
    /// How far short of the outer clipping zone's line the tail stayed at
    /// best, m (0 or less: reached it).
    pub ocz_short_m: f64,
    /// Mean slip angle, degrees.
    pub mean_angle_deg: f64,
    /// Share of the zone spent sliding.
    pub sliding: f64,
    /// Times the car straightened and slid again.
    pub straightened: u32,
    /// The penalty, if any.
    pub penalty: Option<Penalty>,
}

impl ZoneScore {
    /// A short account for the screen.
    pub fn describe(&self) -> String {
        match self.penalty {
            Some(Penalty::Spin) => "spin: no score".into(),
            Some(Penalty::OffCourse) => "off course: no score".into(),
            None => format!(
                "{:.0} deg mean, {:.0} % sliding, ICP {:.1} m, OCZ {}",
                self.mean_angle_deg,
                100.0 * self.sliding,
                self.icp_gap_m,
                if self.ocz_short_m <= 0.0 {
                    "reached".to_string()
                } else {
                    format!("{:.1} m short", self.ocz_short_m)
                }
            ),
        }
    }
}

fn ramp(x: f64, zero: f64, full: f64) -> f64 {
    ((x - zero) / (full - zero)).clamp(0.0, 1.0)
}

/// Scores one zone from the samples taken in it, `dt` seconds apart.
pub fn score_zone(zone: &DriftZoneDef, samples: &[Sample], dt: f64) -> ZoneScore {
    let n = samples.len().max(1) as f64;
    let mean = samples.iter().map(|x| x.slip_deg).sum::<f64>() / n;
    let sliding = samples.iter().filter(|x| x.slip_deg >= SLIDE_DEG).count() as f64 / n;
    // Straightening: from a slide to under STRAIGHT_DEG and back to a slide.
    let mut straightened = 0;
    let (mut was_sliding, mut straight) = (false, false);
    for x in samples {
        if x.slip_deg >= SLIDE_DEG {
            if straight {
                straightened += 1;
            }
            was_sliding = true;
            straight = false;
        } else if was_sliding && x.slip_deg < STRAIGHT_DEG {
            straight = true;
        }
    }
    let icp_gap = samples
        .iter()
        .map(|x| x.icp_gap_m)
        .fold(f64::INFINITY, f64::min);
    // The tail's reach outward, toward the OCZ's side.
    let outward = zone.ocz_offset_m.signum();
    let target = zone.ocz_offset_m.abs();
    let reach = samples
        .iter()
        .filter(|x| x.s >= zone.ocz_from_m && x.s <= zone.ocz_to_m)
        .map(|x| outward * x.tail_offset_m)
        .fold(f64::NEG_INFINITY, f64::max);
    let short = target - reach;
    let icp = 1.0 - ramp(icp_gap, ICP_RANGE_M.0, ICP_RANGE_M.1);
    let ocz = if reach.is_finite() {
        1.0 - ramp(short, 0.0, OCZ_SHORT_M)
    } else {
        0.0
    };
    let line = 0.5 * (icp + ocz);
    let angle = ramp(mean, ANGLE_RANGE_DEG.0, ANGLE_RANGE_DEG.1);
    let style = (sliding - 0.3 * f64::from(straightened)).clamp(0.0, 1.0);
    // Penalties.
    let spun = samples.iter().any(|x| x.slip_deg > SPIN_DEG);
    let mut off = 0.0_f64;
    let mut worst_off = 0.0_f64;
    for x in samples {
        off = if x.on_course { 0.0 } else { off + dt };
        worst_off = worst_off.max(off);
    }
    let penalty = if spun {
        Some(Penalty::Spin)
    } else if worst_off > OFF_COURSE_S {
        Some(Penalty::OffCourse)
    } else {
        None
    };
    let total = if penalty.is_some() || samples.is_empty() {
        0.0
    } else {
        WEIGHTS[0] * line + WEIGHTS[1] * angle + WEIGHTS[2] * style
    };
    ZoneScore {
        name: zone.name.clone(),
        total,
        line,
        angle,
        style,
        icp_gap_m: icp_gap,
        ocz_short_m: if reach.is_finite() {
            short
        } else {
            f64::INFINITY
        },
        mean_angle_deg: mean,
        sliding,
        straightened,
        penalty,
    }
}

/// Where a lap lost the most points, and why (D-118): the corner with the
/// lowest score, and the part of its score that lost most, in words. A
/// corner not passed counts as all points lost. `None` for a lap of 100.
pub fn coach_note(lap: &LapScore, def: &DriftDef) -> Option<String> {
    let (i, lost) = lap
        .zones
        .iter()
        .enumerate()
        .map(|(i, z)| (i, 100.0 - z.as_ref().map_or(0.0, |z| z.total)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if lost < 0.5 {
        return None;
    }
    let name = def.zones.get(i).map_or("?", |z| z.name.as_str());
    let why = match &lap.zones[i] {
        None => "not passed: the corner was not driven through".to_string(),
        Some(z) => zone_reason(z),
    };
    Some(format!("Most points lost: {name} (-{lost:.0}), {why}"))
}

/// The biggest loss inside one judged corner, in words.
fn zone_reason(z: &ZoneScore) -> String {
    match z.penalty {
        Some(Penalty::Spin) => return format!("a spin (over {SPIN_DEG:.0} deg) zeroed it"),
        Some(Penalty::OffCourse) => {
            return format!("off the course for over {OFF_COURSE_S:.1} s zeroed it")
        }
        None => {}
    }
    let icp = 1.0 - ramp(z.icp_gap_m, ICP_RANGE_M.0, ICP_RANGE_M.1);
    let ocz = if z.ocz_short_m.is_finite() {
        1.0 - ramp(z.ocz_short_m, 0.0, OCZ_SHORT_M)
    } else {
        0.0
    };
    let losses = [
        0.5 * WEIGHTS[0] * (1.0 - icp),
        0.5 * WEIGHTS[0] * (1.0 - ocz),
        WEIGHTS[1] * (1.0 - z.angle),
        WEIGHTS[2] * (1.0 - z.style),
    ];
    let worst = (0..4)
        .max_by(|&a, &b| losses[a].total_cmp(&losses[b]))
        .unwrap_or(0);
    let pts = losses[worst];
    match worst {
        0 => format!(
            "line: missed the inner clipping point by {:.1} m (-{pts:.0})",
            (z.icp_gap_m - ICP_RANGE_M.0).max(0.0)
        ),
        1 if z.ocz_short_m.is_finite() => format!(
            "line: the tail stopped {:.1} m short of the outer zone (-{pts:.0})",
            z.ocz_short_m
        ),
        1 => format!("line: the tail never reached the outer zone (-{pts:.0})"),
        2 => format!(
            "angle under target: {:.0} deg mean, full marks at {:.0} (-{pts:.0})",
            z.mean_angle_deg, ANGLE_RANGE_DEG.1
        ),
        _ if z.straightened > 0 => format!(
            "transition not smooth: straightened {} time(s) mid-corner (-{pts:.0})",
            z.straightened
        ),
        _ => format!(
            "style: sliding only {:.0} % of the corner (-{pts:.0})",
            100.0 * z.sliding
        ),
    }
}

/// A judged lap.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LapScore {
    /// One result per zone in lap order; `None` for a zone not passed.
    pub zones: Vec<Option<ZoneScore>>,
    /// Mean of the zones' scores, out of 100.
    pub total: f64,
    /// True if the lap counted (track limits kept, course followed).
    pub counted: bool,
}

/// The judge's state shared with the front end.
#[derive(Clone, Debug, Default)]
pub struct DriftSlot {
    /// True when the track has drift zones.
    pub active: bool,
    /// Current slip angle, degrees.
    pub angle_deg: f64,
    /// Zone the car is in, if any.
    pub zone: Option<String>,
    /// The latest corner's result.
    pub last_zone: Option<ZoneScore>,
    /// Mean of the corners judged so far this lap.
    pub lap_so_far: f64,
    /// Corners judged so far this lap.
    pub zones_so_far: u32,
    /// Counts finished laps, so the front end can tell a new one.
    pub serial: u32,
    /// The latest finished lap.
    pub last_lap: Option<LapScore>,
    /// Best counted lap total.
    pub best_total: Option<f64>,
    /// Where the last lap lost the most points, and why ([`coach_note`]).
    pub coach: Option<String>,
}

/// Judges a session as it is driven. Call [`DriftJudge::observe`] after
/// every tick.
pub struct DriftJudge {
    def: Option<DriftDef>,
    decimation: u32,
    counter: u32,
    dt: f64,
    seen: Option<(u32, u32)>,
    last_s: Option<f64>,
    current: Option<usize>,
    samples: Vec<Sample>,
    lap: Vec<Option<ZoneScore>>,
    slot: Arc<Mutex<DriftSlot>>,
    just_counted: Option<f64>,
}

impl DriftJudge {
    /// A judge for the session's track; inactive on a track with no zones.
    pub fn new(session: &Session) -> Self {
        let def = session.track().def().drift.clone();
        let rate = f64::from(session.tick_rate_hz());
        let decimation = (rate / SAMPLE_HZ).round().max(1.0) as u32;
        let zones = def.as_ref().map_or(0, |d| d.zones.len());
        let slot = DriftSlot {
            active: def.is_some(),
            ..DriftSlot::default()
        };
        Self {
            def,
            decimation,
            counter: 0,
            dt: f64::from(decimation) / rate,
            seen: None,
            last_s: None,
            current: None,
            samples: Vec::new(),
            lap: vec![None; zones],
            slot: Arc::new(Mutex::new(slot)),
            just_counted: None,
        }
    }

    /// The shared state the front end reads.
    pub fn slot(&self) -> Arc<Mutex<DriftSlot>> {
        Arc::clone(&self.slot)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DriftSlot> {
        self.slot.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Reads the session after a tick.
    pub fn observe(&mut self, session: &Session) {
        let Some(def) = self.def.clone() else {
            return;
        };
        self.just_counted = None;
        let t = session.timing();
        let finished = (t.runs, t.invalid_runs);
        if let Some(before) = self.seen {
            if finished != before {
                self.close_zone(&def);
                let counted = finished.0 > before.0;
                self.finish_lap(counted);
            }
        }
        self.seen = Some(finished);
        if !t.running {
            self.close_zone(&def);
            self.lap.iter_mut().for_each(|z| *z = None);
            self.counter = 0;
            return;
        }
        self.counter = (self.counter + 1) % self.decimation;
        if self.counter != 0 {
            return;
        }
        let Some(sample) = self.sample(session, &def) else {
            return;
        };
        self.lock().angle_deg = sample.slip_deg;
        let zone = def
            .zones
            .iter()
            .position(|z| sample.s >= z.from_m && sample.s <= z.to_m);
        if zone != self.current {
            self.close_zone(&def);
            self.current = zone;
            self.lock().zone = zone.map(|i| def.zones[i].name.clone());
        }
        if self.current.is_some() {
            self.samples.push(sample);
        }
    }

    fn sample(&mut self, session: &Session, def: &DriftDef) -> Option<Sample> {
        let track: &Track = session.track();
        let path = track.path(&def.path)?;
        let car = session.car();
        let st = &car.state;
        let len = path.length();
        // Search near where the car was, or the whole path.
        let (from, to) = match self.last_s {
            Some(s) => (s - 40.0, s + 80.0),
            None => (0.0, len),
        };
        let hit = path
            .project(st.pos.x, st.pos.y, from.max(0.0), to.min(len), 40.0)
            .or_else(|| path.project(st.pos.x, st.pos.y, 0.0, len, 40.0));
        let Some((s, _)) = hit else {
            self.last_s = None;
            return None;
        };
        self.last_s = Some(s);
        let out = car.output();
        let v = out.vel_body;
        let slip = if out.speed < MIN_SPEED {
            0.0
        } else {
            v[1].atan2(v[0]).to_degrees().abs().min(180.0)
        };
        let b = &car.params().body;
        let (cx, hl, hw) = (b.box_center_x(), 0.5 * b.length_m, 0.5 * b.width_m);
        let world = |x: f64, y: f64| st.pos + st.rot.rotate(Vec3::new(x, y, 0.0));
        let zone = def
            .zones
            .iter()
            .find(|z| s >= z.from_m && s <= z.to_m)
            .or_else(|| def.zones.first())?;
        let icp = path_point(path, zone.icp_at_m, zone.icp_offset_m);
        let icp_gap = [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)]
            .iter()
            .map(|(a, c)| {
                let p = world(cx + a * hl, c * hw);
                (p.x - icp[0]).hypot(p.y - icp[1])
            })
            .fold(f64::INFINITY, f64::min);
        let tail = world(cx - hl, 0.0);
        let tail_offset = path
            .project(
                tail.x,
                tail.y,
                (s - 30.0).max(0.0),
                (s + 30.0).min(len),
                40.0,
            )
            .map_or(0.0, |(_, o)| o);
        let on_course = out.wheels.iter().any(|w| {
            let [x, y, z] = w.center_body;
            let c = st.pos + st.rot.rotate(Vec3::new(x, y, z));
            !track.has_limits() || track.within_limits(c.x, c.y)
        });
        Some(Sample {
            s,
            slip_deg: slip,
            icp_gap_m: icp_gap,
            tail_offset_m: tail_offset,
            on_course,
        })
    }

    fn close_zone(&mut self, def: &DriftDef) {
        let Some(i) = self.current.take() else {
            return;
        };
        let samples = std::mem::take(&mut self.samples);
        if samples.is_empty() {
            return;
        }
        let score = score_zone(&def.zones[i], &samples, self.dt);
        self.lap[i] = Some(score.clone());
        let judged: Vec<f64> = self.lap.iter().flatten().map(|z| z.total).collect();
        let mut s = self.lock();
        s.zone = None;
        s.lap_so_far = judged.iter().sum::<f64>() / judged.len().max(1) as f64;
        s.zones_so_far = judged.len() as u32;
        s.last_zone = Some(score);
    }

    /// The score of a counted lap finished on the tick just observed; for
    /// the ghost of the best-scored lap.
    pub fn just_counted(&self) -> Option<f64> {
        self.just_counted
    }

    fn finish_lap(&mut self, counted: bool) {
        let n = self.lap.len();
        let total = self.lap.iter().flatten().map(|z| z.total).sum::<f64>() / n.max(1) as f64;
        let lap = LapScore {
            zones: std::mem::replace(&mut self.lap, vec![None; n]),
            total,
            counted,
        };
        let coach = self.def.as_ref().and_then(|d| coach_note(&lap, d));
        if counted {
            self.just_counted = Some(total);
        }
        let mut s = self.lock();
        s.serial += 1;
        s.coach = coach;
        if counted {
            s.best_total = Some(s.best_total.map_or(total, |b| b.max(total)));
        }
        s.last_lap = Some(lap);
        s.lap_so_far = 0.0;
        s.zones_so_far = 0;
    }
}

/// The world point at `at` along a path, `offset` to its left.
pub fn path_point(path: &crate::track::PathGeom, at: f64, offset: f64) -> [f64; 2] {
    let (p, heading) = path.at(at);
    [p[0] - offset * heading.sin(), p[1] + offset * heading.cos()]
}
