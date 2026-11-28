// SPDX-License-Identifier: Apache-2.0
//! Drift judging (D-114): each judged corner of a drift track is scored as
//! formal drift judging scores a run, on line, angle and style, from what
//! the car did through it. It only reads the session.
//!
//! Weights and rules as Formula DRIFT's 2026 qualifying (D-119):
//! - **Line** (40 points): how close the car comes to the inner clipping
//!   point (ICP), and how far its tail reaches into the outer clipping zone
//!   (OCZ), half each.
//! - **Angle** (40 points): the mean body slip angle over the maximum,
//!   [`ANGLE_RANGE_DEG`]; full marks at 35 degrees or more.
//! - **Style** (20 points): the share of the zone spent sliding (10
//!   degrees or more), less 30 percent of the marks for every time the car
//!   straightens and slides again within it.
//! - **Incomplete**, which scores nothing for the run: a spin, three tyres
//!   off, or an opposite drift (the constants below).
//!
//! A run's score is taken over the whole run; each corner also gets its own
//! score for the panel and the coach.

use crate::session::Session;
use crate::track::{DriftDef, DriftZoneDef, Track};
use qts_core::math::Vec3;
use qts_core::step::Simulation;
use std::sync::{Arc, Mutex};

/// Weights of line, angle and style, out of 100: Formula DRIFT's 2026
/// qualifying split (Judging Regulations 2026 v1.3, 3.1; D-119).
pub const WEIGHTS: [f64; 3] = [40.0, 40.0, 20.0];
/// Slip angle that counts as sliding, degrees.
pub const SLIDE_DEG: f64 = 10.0;
/// Below this the car is taken to have straightened, degrees.
pub const STRAIGHT_DEG: f64 = 5.0;
/// Mean slip angle for no angle marks, and for full marks, degrees. As
/// in the rulebook, the score is the average angle over a maximum, with
/// no floor; the rulebook sets the maximum per event from the field's
/// data, this project fixes it at 35.
///
/// UNVERIFIED (D-133): no published figure backs 35. Formula DRIFT judges
/// angle against each event's own maximum, and describes angle only as
/// the most a driver can hold under control; no rulebook found names a
/// number of degrees. 35 is our own choice for these fictional cars.
pub const ANGLE_RANGE_DEG: (f64, f64) = (0.0, 35.0);
/// Slip angle that is a spin, degrees: the rulebook's "approximately 180
/// degrees" of over-rotation, taken from 150.
pub const SPIN_DEG: f64 = 150.0;
/// Tyres off the course at once that make the run Incomplete (rulebook
/// 3.2: three tyres off in the zones).
pub const TYRES_OFF_INCOMPLETE: u8 = 3;
/// Time sliding the wrong way for the corner that makes the run
/// Incomplete ("opposite drift"), s. The rulebook gives no time; this
/// leaves room for a transition at a zone's edge.
pub const OPPOSITE_S: f64 = 0.5;
/// Past the outer zone's line the tail is in the "No-Go" area, which the
/// rulebook deducts from the zone's fill. The deduction starts this far
/// past the line (the line's width and the bumper's curve) and takes the
/// whole outer half at [`OCZ_SHORT_M`] further, m.
pub const NO_GO_FROM_M: f64 = 0.5;
/// Gap to the inner clipping point for full marks, and for none, m.
pub const ICP_RANGE_M: (f64, f64) = (0.5, 4.0);
/// How far short of the outer clipping zone's line the tail may stay and
/// still score anything, m.
pub const OCZ_SHORT_M: f64 = 4.0;
/// Below this speed slip angle means nothing, m/s.
pub const MIN_SPEED: f64 = 5.0;
/// Rows a second the judge keeps.
pub const SAMPLE_HZ: f64 = 100.0;

// Why a drift spun (D-127). These thresholds are this project's own, set
// against what the model does; none is from a published source.
/// Body slip past which the spin is taken to have begun: beyond it the
/// computer driver (D-126) never caught a slide on Harrow, degrees.
/// Unverified as a general limit.
pub const SPIN_ONSET_DEG: f64 = 60.0;
/// How far before that the coach looks for a cause, s.
pub const SPIN_LOOKBACK_S: f64 = 1.5;
/// Steering at or past this share of full lock means the front could not
/// turn further to catch the slide.
pub const LOCK_SHARE: f64 = 0.97;
/// Throttle rise within [`THROTTLE_RISE_S`] that counts as too sudden:
/// half the pedal in a fifth of a second. Unverified.
pub const THROTTLE_RISE: f64 = 0.5;
/// See [`THROTTLE_RISE`], s.
pub const THROTTLE_RISE_S: f64 = 0.2;

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
    /// Tyres off the course.
    pub tyres_off: u8,
    /// True when sliding (over [`SLIDE_DEG`]) the wrong way for the corner.
    pub wrong_way: bool,
    /// Steering wheel angle as a share of full lock, 0 to 1.
    pub steer_lock: f64,
    /// Accelerator pedal, 0 to 1.
    pub throttle: f64,
    /// Larger slip angle of the two rear tyres, degrees.
    pub rear_slip_deg: f64,
}

/// The likely reason a drift spun, read from the samples before it (D-127).
#[derive(Clone, Debug, PartialEq)]
pub enum SpinCause {
    /// The steering reached full lock while the slide still grew.
    SteeringLock {
        /// Slip angle when lock was reached, degrees.
        slip_deg: f64,
    },
    /// The throttle went on faster than the rear could take.
    ThrottleSudden {
        /// Pedal before and after the rise.
        from: f64,
        /// See `from`.
        to: f64,
    },
    /// Neither: the rear slid further than the car could be caught from.
    RearSlipPastLimit {
        /// Largest rear tyre slip angle before the spin began, degrees.
        rear_slip_deg: f64,
    },
}

impl SpinCause {
    /// In words, for the coach.
    pub fn describe(&self) -> String {
        match self {
            SpinCause::SteeringLock { slip_deg } => format!(
                "steering lock reached at {slip_deg:.0} deg of slide: the front could not turn further to catch it; less angle or less speed on entry"
            ),
            SpinCause::ThrottleSudden { from, to } => format!(
                "throttle too sudden: {:.0} % to {:.0} % in {:.1} s spun the rears up; feed it in",
                100.0 * from,
                100.0 * to,
                THROTTLE_RISE_S
            ),
            SpinCause::RearSlipPastLimit { rear_slip_deg } => format!(
                "rear slip past the limit: the rear tyres reached {rear_slip_deg:.0} deg with lock and throttle to spare; ease off sooner as the angle grows"
            ),
        }
    }
}

/// The likely cause of a spin in `samples` (taken `dt` apart), or `None`
/// if the slide never passed [`SPIN_ONSET_DEG`].
pub fn spin_cause(samples: &[Sample], dt: f64) -> Option<SpinCause> {
    let onset = samples.iter().position(|x| x.slip_deg > SPIN_ONSET_DEG)?;
    let back = (SPIN_LOOKBACK_S / dt).round() as usize;
    let window = &samples[onset.saturating_sub(back)..=onset];
    if let Some(x) = window.iter().find(|x| x.steer_lock >= LOCK_SHARE) {
        return Some(SpinCause::SteeringLock {
            slip_deg: x.slip_deg,
        });
    }
    let gap = ((THROTTLE_RISE_S / dt).round() as usize).max(1);
    let rise = window
        .windows(gap + 1)
        .map(|w| (w[0].throttle, w[gap].throttle))
        .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)));
    if let Some((from, to)) = rise {
        if to - from >= THROTTLE_RISE {
            return Some(SpinCause::ThrottleSudden { from, to });
        }
    }
    let rear = window.iter().map(|x| x.rear_slip_deg).fold(0.0, f64::max);
    Some(SpinCause::RearSlipPastLimit {
        rear_slip_deg: rear,
    })
}

/// Why a run is Incomplete: it scores nothing (rulebook 3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Penalty {
    /// More than [`SPIN_DEG`] of slip.
    Spin,
    /// [`TYRES_OFF_INCOMPLETE`] or more tyres off the course.
    OffCourse,
    /// Sliding the wrong way for the corner for [`OPPOSITE_S`] or more.
    Opposite,
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
    /// How far past the outer zone's line the tail went, m (0 if not).
    pub no_go_m: f64,
    /// Samples taken in the zone.
    pub samples: usize,
    /// Mean slip angle, degrees.
    pub mean_angle_deg: f64,
    /// Share of the zone spent sliding.
    pub sliding: f64,
    /// Times the car straightened and slid again.
    pub straightened: u32,
    /// The penalty, if any.
    pub penalty: Option<Penalty>,
    /// For a spin: its likely cause.
    pub spin_cause: Option<SpinCause>,
}

impl ZoneScore {
    /// A short account for the screen.
    pub fn describe(&self) -> String {
        match self.penalty {
            Some(Penalty::Spin) => "spin: run Incomplete".into(),
            Some(Penalty::OffCourse) => "three tyres off: run Incomplete".into(),
            Some(Penalty::Opposite) => "opposite drift: run Incomplete".into(),
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

/// Share of the outer zone filled, from how far short of its line the tail
/// stayed (negative: past it, into the No-Go area, which is deducted).
fn outer_fill(short: f64) -> f64 {
    if short >= 0.0 {
        1.0 - ramp(short, 0.0, OCZ_SHORT_M)
    } else {
        1.0 - ramp(-short, NO_GO_FROM_M, NO_GO_FROM_M + OCZ_SHORT_M)
    }
}

/// A lap's score out of 100 the rulebook's way (3.1): line is the mean
/// fill over all zones (a zone not passed fills nothing), angle the mean
/// slip over every judged sample against the maximum, style the mean over
/// the zones; any Incomplete scores nothing.
pub fn lap_total(zones: &[Option<ZoneScore>]) -> f64 {
    let n = zones.len().max(1) as f64;
    if zones.iter().flatten().any(|z| z.penalty.is_some()) {
        return 0.0;
    }
    let line = zones.iter().flatten().map(|z| z.line).sum::<f64>() / n;
    let style = zones.iter().flatten().map(|z| z.style).sum::<f64>() / n;
    let samples: usize = zones.iter().flatten().map(|z| z.samples).sum();
    let angle_sum: f64 = zones
        .iter()
        .flatten()
        .map(|z| z.mean_angle_deg * z.samples as f64)
        .sum();
    let mean_angle = angle_sum / samples.max(1) as f64;
    let angle = ramp(mean_angle, ANGLE_RANGE_DEG.0, ANGLE_RANGE_DEG.1);
    WEIGHTS[0] * line + WEIGHTS[1] * angle + WEIGHTS[2] * style
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
    let no_go = (-short).max(0.0);
    let icp = 1.0 - ramp(icp_gap, ICP_RANGE_M.0, ICP_RANGE_M.1);
    let ocz = if reach.is_finite() {
        outer_fill(short)
    } else {
        0.0
    };
    let line = 0.5 * (icp + ocz);
    let angle = ramp(mean, ANGLE_RANGE_DEG.0, ANGLE_RANGE_DEG.1);
    let style = (sliding - 0.3 * f64::from(straightened)).clamp(0.0, 1.0);
    // Penalties.
    let spun = samples.iter().any(|x| x.slip_deg > SPIN_DEG);
    let off = samples.iter().any(|x| x.tyres_off >= TYRES_OFF_INCOMPLETE);
    let (mut wrong, mut worst_wrong) = (0.0_f64, 0.0_f64);
    for x in samples {
        wrong = if x.wrong_way { wrong + dt } else { 0.0 };
        worst_wrong = worst_wrong.max(wrong);
    }
    let penalty = if spun {
        Some(Penalty::Spin)
    } else if off {
        Some(Penalty::OffCourse)
    } else if worst_wrong >= OPPOSITE_S - 1e-9 {
        Some(Penalty::Opposite)
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
        no_go_m: if reach.is_finite() { no_go } else { 0.0 },
        spin_cause: if spun { spin_cause(samples, dt) } else { None },
        samples: samples.len(),
        mean_angle_deg: mean,
        sliding,
        straightened,
        penalty,
    }
}

/// Where a lap lost the most points, and why (D-118): the corner with the
/// lowest score, and the part of its score that lost most, in words. A
/// corner not passed counts as all points lost; a corner that made the run
/// Incomplete comes first. `None` for a lap of 100.
pub fn coach_note(lap: &LapScore, def: &DriftDef) -> Option<String> {
    let incomplete = lap
        .zones
        .iter()
        .position(|z| z.as_ref().is_some_and(|z| z.penalty.is_some()));
    let (i, lost) = match incomplete {
        Some(i) => (i, 100.0),
        None => lap
            .zones
            .iter()
            .enumerate()
            .map(|(i, z)| (i, 100.0 - z.as_ref().map_or(0.0, |z| z.total)))
            .max_by(|a, b| a.1.total_cmp(&b.1))?,
    };
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
        Some(Penalty::Spin) => {
            let why = z.spin_cause.as_ref().map_or(String::new(), |c| {
                format!(". Likely cause: {}", c.describe())
            });
            return format!("a spin (over {SPIN_DEG:.0} deg) made the run Incomplete{why}");
        }
        Some(Penalty::OffCourse) => {
            return format!("{TYRES_OFF_INCOMPLETE} tyres off the course made the run Incomplete")
        }
        Some(Penalty::Opposite) => return "an opposite drift made the run Incomplete".to_string(),
        None => {}
    }
    let icp = 1.0 - ramp(z.icp_gap_m, ICP_RANGE_M.0, ICP_RANGE_M.1);
    let ocz = if z.ocz_short_m.is_finite() {
        outer_fill(z.ocz_short_m)
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
        1 if z.no_go_m > NO_GO_FROM_M => format!(
            "line: the tail went {:.1} m past the outer zone into the No-Go area (-{pts:.0})",
            z.no_go_m
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
    /// Tandem judging while chasing a lead car (D-137).
    pub tandem: Option<crate::tandem::TandemState>,
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
    /// Rear grip the setup gives away against the safe baseline (D-127).
    setup_notes: Vec<String>,
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
            setup_notes: session.setup_notes().to_vec(),
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
        // Sliding the wrong way: the body slip's side is the corner's side
        // (a right-hander, ICP on the right, slides with the car moving to
        // the left of where it points).
        let wrong_way = slip >= SLIDE_DEG && v[1].signum() == zone.icp_offset_m.signum();
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
        let tyres_off = out
            .wheels
            .iter()
            .filter(|w| {
                let [x, y, z] = w.center_body;
                let c = st.pos + st.rot.rotate(Vec3::new(x, y, z));
                track.has_limits() && !track.within_limits(c.x, c.y)
            })
            .count() as u8;
        Some(Sample {
            s,
            slip_deg: slip,
            icp_gap_m: icp_gap,
            tail_offset_m: tail_offset,
            tyres_off,
            wrong_way,
            steer_lock: (out.steering_wheel_angle.abs()
                / car.params().steering.max_wheel_angle_rad.max(1e-6))
            .min(1.0),
            throttle: session.controls().throttle,
            rear_slip_deg: out.wheels[2]
                .slip_angle
                .abs()
                .max(out.wheels[3].slip_angle.abs())
                .to_degrees(),
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
        let judged: Vec<Option<ZoneScore>> =
            self.lap.iter().filter(|z| z.is_some()).cloned().collect();
        let mut s = self.lock();
        s.zone = None;
        s.lap_so_far = lap_total(&judged);
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
        let total = lap_total(&self.lap);
        let lap = LapScore {
            zones: std::mem::replace(&mut self.lap, vec![None; n]),
            total,
            counted,
        };
        let mut coach = self.def.as_ref().and_then(|d| coach_note(&lap, d));
        // A spin: the setup's part too, when it gives rear grip away.
        let spun = lap
            .zones
            .iter()
            .any(|z| z.as_ref().is_some_and(|z| z.penalty == Some(Penalty::Spin)));
        if let (true, Some(note), false) = (spun, coach.as_mut(), self.setup_notes.is_empty()) {
            note.push_str(&format!(
                ". The setup also gives rear grip away against the safe drift baseline: {}",
                self.setup_notes.join("; ")
            ));
        }
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
