// SPDX-License-Identifier: Apache-2.0
//! Tandem judging (D-137): chasing the computer's lead car through a drift
//! track's judged zones, the chase car is scored on how close it stays and
//! how well it copies the lead's angle and line, as tandem judging asks of
//! a chase driver. It only reads the two sessions.
//!
//! Per zone, out of 100 (the weights are this project's, **unverified**:
//! the rules describe what is judged, not a points split):
//! - **Proximity** ([`WEIGHTS`]\[0\]): the gap between the two cars'
//!   bodies, scored by [`PROXIMITY_BANDS`] and averaged over the zone.
//! - **Angle** ([`WEIGHTS`]\[1\]): how close the chase car's slip angle is
//!   to the lead's where it was at the same point of the track.
//! - **Line** ([`WEIGHTS`]\[2\]): how close the chase car runs to where the
//!   lead ran across the track at the same point along it.
//! - **Contact** costs [`CONTACT_PENALTY`] points each time the bodies
//!   touch. The cars do not collide (D-122), so contact is the two
//!   footprints overlapping.
//! - **Lost the lead**: more than [`LOST_GAP_M`] behind for [`LOST_S`]
//!   inside a zone scores that zone nothing.
//!
//! The cost is a few dozen arithmetic operations at 100 samples a second,
//! off the physics tick's own work.

use crate::drift::{DriftSlot, SAMPLE_HZ};
use crate::session::Session;
use crate::track::DriftDef;
use qts_core::math::Vec3;
use qts_core::step::Simulation;
use std::sync::{Arc, Mutex};

/// Proximity, angle and line, out of 100. UNVERIFIED (D-137).
pub const WEIGHTS: [f64; 3] = [50.0, 30.0, 20.0];
/// Body-to-body gap bands, m, and the share of the proximity marks each
/// earns; past the last band, nothing. UNVERIFIED (D-137).
pub const PROXIMITY_BANDS: [(f64, f64); 4] = [(1.5, 1.0), (3.0, 0.8), (6.0, 0.5), (10.0, 0.2)];
/// Slip angle difference that scores no angle marks, degrees.
pub const ANGLE_MATCH_DEG: f64 = 20.0;
/// Line difference that scores no line marks, m.
pub const LINE_MATCH_M: f64 = 3.0;
/// Points off for each contact.
pub const CONTACT_PENALTY: f64 = 20.0;
/// A gap over this for [`LOST_S`] inside a zone loses the lead, m.
pub const LOST_GAP_M: f64 = 15.0;
/// How long the gap must stay over [`LOST_GAP_M`], s.
pub const LOST_S: f64 = 1.0;

/// One judged zone of a tandem lap.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TandemZone {
    /// Zone name.
    pub name: String,
    /// Score out of 100.
    pub total: f64,
    /// Share of the proximity marks earned, 0 to 1.
    pub proximity: f64,
    /// Share of the angle marks earned, 0 to 1.
    pub angle: f64,
    /// Share of the line marks earned, 0 to 1.
    pub line: f64,
    /// Mean body-to-body gap, m.
    pub mean_gap_m: f64,
    /// Mean slip angle difference, degrees.
    pub mean_angle_diff_deg: f64,
    /// Mean line difference, m.
    pub mean_line_diff_m: f64,
    /// Times the bodies touched.
    pub contacts: u32,
    /// True if the chase car lost the lead in this zone.
    pub lost: bool,
}

impl TandemZone {
    /// The biggest loss in words, for the coach.
    pub fn reason(&self) -> String {
        if self.lost {
            return format!("lost the lead (over {LOST_GAP_M:.0} m behind for {LOST_S:.0} s)");
        }
        let losses = [
            WEIGHTS[0] * (1.0 - self.proximity),
            WEIGHTS[1] * (1.0 - self.angle),
            WEIGHTS[2] * (1.0 - self.line),
            CONTACT_PENALTY * f64::from(self.contacts),
        ];
        let worst = (0..4)
            .max_by(|&a, &b| losses[a].total_cmp(&losses[b]))
            .unwrap_or(0);
        let pts = losses[worst];
        match worst {
            0 => format!(
                "proximity: {:.1} m behind on average (-{pts:.0})",
                self.mean_gap_m
            ),
            1 => format!(
                "angle: {:.0} deg off the lead's on average (-{pts:.0})",
                self.mean_angle_diff_deg
            ),
            2 => format!(
                "line: {:.1} m off the lead's line on average (-{pts:.0})",
                self.mean_line_diff_m
            ),
            _ => format!(
                "contact with the lead {} time(s) (-{pts:.0})",
                self.contacts
            ),
        }
    }
}

/// `s` along a path of length `len`, counted on from `near` (the
/// previous count) to whichever lap puts it closest.
fn unwrap(s: f64, near: Option<f64>, len: f64) -> f64 {
    near.map_or(s, |n| s + len * ((n - s) / len).round())
}

/// Share of the proximity marks for a gap, m.
pub fn proximity_share(gap_m: f64) -> f64 {
    PROXIMITY_BANDS
        .iter()
        .find(|(upto, _)| gap_m <= *upto)
        .map_or(0.0, |(_, share)| *share)
}

/// A car's footprint on the ground: centre, unit forward axis, half length
/// and half width.
#[derive(Clone, Copy, Debug)]
pub struct Footprint {
    /// Centre, m.
    pub centre: [f64; 2],
    /// Unit vector along the car.
    pub forward: [f64; 2],
    /// Half the body length, m.
    pub half_length: f64,
    /// Half the body width, m.
    pub half_width: f64,
}

impl Footprint {
    fn of(session: &Session) -> Self {
        let car = session.car();
        let st = &car.state;
        let b = &car.params().body;
        let c = st.pos + st.rot.rotate(Vec3::new(b.box_center_x(), 0.0, 0.0));
        let f = st.rot.rotate(Vec3::new(1.0, 0.0, 0.0));
        let n = f.x.hypot(f.y).max(1e-9);
        Self {
            centre: [c.x, c.y],
            forward: [f.x / n, f.y / n],
            half_length: 0.5 * b.length_m,
            half_width: 0.5 * b.width_m,
        }
    }

    fn corners(&self) -> [[f64; 2]; 4] {
        let [fx, fy] = self.forward;
        let (lx, ly) = (-fy, fx);
        let mut out = [[0.0; 2]; 4];
        for (k, (a, c)) in [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)]
            .iter()
            .enumerate()
        {
            out[k] = [
                self.centre[0] + a * self.half_length * fx + c * self.half_width * lx,
                self.centre[1] + a * self.half_length * fy + c * self.half_width * ly,
            ];
        }
        out
    }

    /// Distance from a point to this footprint, m; 0 inside.
    fn distance_to(&self, p: [f64; 2]) -> f64 {
        let (dx, dy) = (p[0] - self.centre[0], p[1] - self.centre[1]);
        let [fx, fy] = self.forward;
        let along = (dx * fx + dy * fy).abs() - self.half_length;
        let across = (-dx * fy + dy * fx).abs() - self.half_width;
        along.max(0.0).hypot(across.max(0.0))
    }

    /// Gap between two footprints, m: 0 when they touch or overlap. Exact
    /// for rectangles apart (the nearest points include a corner) except
    /// edge-crossing overlaps, which [`Footprint::overlaps`] catches.
    pub fn gap(&self, other: &Footprint) -> f64 {
        let a = self.corners().into_iter().map(|p| other.distance_to(p));
        let b = other.corners().into_iter().map(|p| self.distance_to(p));
        a.chain(b).fold(f64::INFINITY, f64::min)
    }

    /// True if the two footprints overlap (separating axis test).
    pub fn overlaps(&self, other: &Footprint) -> bool {
        let axes = [
            self.forward,
            [-self.forward[1], self.forward[0]],
            other.forward,
            [-other.forward[1], other.forward[0]],
        ];
        let (ca, cb) = (self.corners(), other.corners());
        axes.iter().all(|ax| {
            let proj = |c: &[[f64; 2]; 4]| {
                c.iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                        let d = p[0] * ax[0] + p[1] * ax[1];
                        (lo.min(d), hi.max(d))
                    })
            };
            let (a, b) = (proj(&ca), proj(&cb));
            a.0 <= b.1 && b.0 <= a.1
        })
    }
}

/// The share of the tandem judge the front end reads, kept in the drift
/// judge's slot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TandemState {
    /// The latest zone's result.
    pub last_zone: Option<TandemZone>,
    /// Mean of the zones judged so far this lap.
    pub lap_so_far: f64,
    /// The latest finished lap, per zone.
    pub last_lap: Vec<Option<TandemZone>>,
    /// Mean of the latest finished lap's zones, out of 100.
    pub last_lap_total: f64,
    /// The zone that cost the most, in words.
    pub coach: Option<String>,
    /// Live gap to the lead, m.
    pub gap_m: f64,
}

#[derive(Default)]
struct Acc {
    n: u32,
    prox: f64,
    angle: f64,
    line: f64,
    gap: f64,
    angle_diff: f64,
    line_diff: f64,
    contacts: u32,
    touching: bool,
    behind_s: f64,
    lost: bool,
}

/// Judges the chase car against the lead. Call [`TandemJudge::observe`]
/// after every tick, once both sessions have stepped.
pub struct TandemJudge {
    def: Option<DriftDef>,
    decimation: u32,
    counter: u32,
    dt: f64,
    seen: Option<(u32, u32)>,
    chase_s: Option<f64>,
    lead_s: Option<f64>,
    /// The lead's (distance along the path, offset across it, slip angle
    /// in degrees), in order.
    lead_line: Vec<(f64, f64, f64)>,
    current: Option<usize>,
    acc: Acc,
    lap: Vec<Option<TandemZone>>,
    slot: Arc<Mutex<DriftSlot>>,
}

impl TandemJudge {
    /// A judge for the chase car's track, writing into the drift judge's
    /// slot; inactive on a track with no zones.
    pub fn new(chase: &Session, slot: Arc<Mutex<DriftSlot>>) -> Self {
        let def = chase.track().def().drift.clone();
        let rate = f64::from(chase.tick_rate_hz());
        let decimation = (rate / SAMPLE_HZ).round().max(1.0) as u32;
        let zones = def.as_ref().map_or(0, |d| d.zones.len());
        if let Ok(mut s) = slot.lock() {
            s.tandem = Some(TandemState::default());
        }
        Self {
            def,
            decimation,
            counter: 0,
            dt: f64::from(decimation) / rate,
            seen: None,
            chase_s: None,
            lead_s: None,
            lead_line: Vec::new(),
            current: None,
            acc: Acc::default(),
            lap: vec![None; zones],
            slot,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DriftSlot> {
        self.slot.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn project(session: &Session, def: &DriftDef, near: Option<f64>) -> Option<(f64, f64)> {
        let path = session.track().path(&def.path)?;
        let p = session.car().state.pos;
        let len = path.length();
        let (from, to) = near.map_or((0.0, len), |s| (s - 40.0, s + 80.0));
        path.project(p.x, p.y, from.max(0.0), to.min(len), 40.0)
            .or_else(|| path.project(p.x, p.y, 0.0, len, 40.0))
    }

    fn slip_deg(session: &Session) -> f64 {
        let v = session.car().output().vel_body;
        // Signed, so a lead sliding one way and a chase sliding the other
        // differ by the sum.
        v[1].atan2(v[0].abs().max(0.5)).to_degrees()
    }

    /// The lead's offset across the track, m, and slip angle, degrees,
    /// when it was at `s`.
    fn lead_at(&self, s: f64) -> Option<(f64, f64)> {
        let i = self.lead_line.partition_point(|p| p.0 < s);
        if i == 0 || i >= self.lead_line.len() {
            return None;
        }
        let (a, b) = (self.lead_line[i - 1], self.lead_line[i]);
        let k = ((s - a.0) / (b.0 - a.0).max(1e-9)).clamp(0.0, 1.0);
        Some((a.1 + k * (b.1 - a.1), a.2 + k * (b.2 - a.2)))
    }

    /// Reads both cars after a tick.
    pub fn observe(&mut self, chase: &Session, lead: &Session) {
        let Some(def) = self.def.clone() else {
            return;
        };
        let t = chase.timing();
        let finished = (t.runs, t.invalid_runs);
        if self.seen.is_some_and(|before| before != finished) {
            self.close_zone(&def);
            self.finish_lap();
        }
        self.seen = Some(finished);
        self.counter = (self.counter + 1) % self.decimation;
        if self.counter != 0 {
            return;
        }
        let len = chase
            .track()
            .path(&def.path)
            .map_or(1.0, |p| p.length())
            .max(1.0);
        let a = Footprint::of(chase);
        let b = Footprint::of(lead);
        let touching = a.overlaps(&b);
        let gap = if touching { 0.0 } else { a.gap(&b) };
        self.lock()
            .tandem
            .get_or_insert_with(TandemState::default)
            .gap_m = gap;
        let Some((cs, co)) = Self::project(chase, &def, self.chase_s.map(|c| c.rem_euclid(len)))
        else {
            self.chase_s = None;
            return;
        };
        // Distances along the path count on over laps, so the chase car is
        // compared with where the lead was, across the start line too.
        let cum_chase = unwrap(cs, self.chase_s, len);
        self.chase_s = Some(cum_chase);
        if let Some((ls, lo)) = Self::project(lead, &def, self.lead_s.map(|c| c.rem_euclid(len))) {
            let cum_lead = unwrap(ls, Some(self.lead_s.unwrap_or(cum_chase)), len);
            self.lead_s = Some(cum_lead);
            if self.lead_line.last().is_none_or(|p| cum_lead > p.0) {
                self.lead_line.push((cum_lead, lo, Self::slip_deg(lead)));
            }
            let keep_from = cum_chase - 50.0;
            let drop = self
                .lead_line
                .partition_point(|p| p.0 < keep_from)
                .saturating_sub(1);
            if drop > 0 {
                self.lead_line.drain(..drop);
            }
        }
        if !t.running {
            self.close_zone(&def);
            return;
        }
        let zone = def
            .zones
            .iter()
            .position(|z| cs >= z.from_m && cs <= z.to_m);
        if zone != self.current {
            self.close_zone(&def);
            self.current = zone;
        }
        if self.current.is_none() {
            return;
        }
        // Both compared with the lead where it was at this point of the
        // track: copying its angle and line, whatever the gap. Where it has
        // not been yet (alongside, or passed), its angle now, and no line
        // marks lost.
        let chase_slip = Self::slip_deg(chase);
        let (angle_diff, line_diff) = match self.lead_at(cum_chase) {
            Some((offset, slip)) => ((chase_slip - slip).abs(), (co - offset).abs()),
            None => ((chase_slip - Self::slip_deg(lead)).abs(), 0.0),
        };
        let dt = self.dt;
        let acc = &mut self.acc;
        acc.n += 1;
        acc.gap += gap;
        acc.prox += proximity_share(gap);
        acc.angle_diff += angle_diff;
        acc.angle += (1.0 - angle_diff / ANGLE_MATCH_DEG).max(0.0);
        acc.line_diff += line_diff;
        acc.line += (1.0 - line_diff / LINE_MATCH_M).max(0.0);
        if touching && !acc.touching {
            acc.contacts += 1;
        }
        acc.touching = touching;
        acc.behind_s = if gap > LOST_GAP_M {
            acc.behind_s + dt
        } else {
            0.0
        };
        if acc.behind_s >= LOST_S {
            acc.lost = true;
        }
    }

    fn close_zone(&mut self, def: &DriftDef) {
        let Some(i) = self.current.take() else {
            return;
        };
        let acc = std::mem::take(&mut self.acc);
        if acc.n == 0 {
            return;
        }
        let n = f64::from(acc.n);
        let mut z = TandemZone {
            name: def.zones[i].name.clone(),
            proximity: acc.prox / n,
            angle: acc.angle / n,
            line: acc.line / n,
            mean_gap_m: acc.gap / n,
            mean_angle_diff_deg: acc.angle_diff / n,
            mean_line_diff_m: acc.line_diff / n,
            contacts: acc.contacts,
            lost: acc.lost,
            total: 0.0,
        };
        z.total = if z.lost {
            0.0
        } else {
            (WEIGHTS[0] * z.proximity + WEIGHTS[1] * z.angle + WEIGHTS[2] * z.line
                - CONTACT_PENALTY * f64::from(z.contacts))
            .max(0.0)
        };
        self.lap[i] = Some(z.clone());
        let judged: Vec<f64> = self.lap.iter().flatten().map(|z| z.total).collect();
        let mut s = self.lock();
        let st = s.tandem.get_or_insert_with(TandemState::default);
        st.lap_so_far = judged.iter().sum::<f64>() / judged.len().max(1) as f64;
        st.last_zone = Some(z);
    }

    fn finish_lap(&mut self) {
        let zones = self.lap.len();
        let lap = std::mem::replace(&mut self.lap, vec![None; zones]);
        if lap.iter().all(|z| z.is_none()) {
            return;
        }
        let total = lap
            .iter()
            .map(|z| z.as_ref().map_or(0.0, |z| z.total))
            .sum::<f64>()
            / lap.len().max(1) as f64;
        let coach = lap
            .iter()
            .enumerate()
            .map(|(i, z)| (i, 100.0 - z.as_ref().map_or(0.0, |z| z.total), z))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|w| w.1 >= 0.5)
            .map(|(i, lost, z)| {
                let name = self
                    .def
                    .as_ref()
                    .and_then(|d| d.zones.get(i))
                    .map_or("?", |z| z.name.as_str());
                let why = z
                    .as_ref()
                    .map_or("not passed".to_string(), TandemZone::reason);
                format!("Tandem, most points lost: {name} (-{lost:.0}), {why}")
            });
        let mut s = self.lock();
        let st = s.tandem.get_or_insert_with(TandemState::default);
        st.last_lap = lap;
        st.last_lap_total = total;
        st.coach = coach;
        st.lap_so_far = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(x: f64, y: f64, heading_deg: f64) -> Footprint {
        let h = heading_deg.to_radians();
        Footprint {
            centre: [x, y],
            forward: [h.cos(), h.sin()],
            half_length: 2.2,
            half_width: 0.9,
        }
    }

    #[test]
    fn the_gap_is_body_to_body() {
        // Nose to tail, 1 m apart.
        let a = fp(0.0, 0.0, 0.0);
        let b = fp(5.4, 0.0, 0.0);
        assert!((a.gap(&b) - 1.0).abs() < 1e-9);
        assert!(!a.overlaps(&b));
        // Sideways and touching.
        let c = fp(3.0, 0.0, 90.0);
        assert!(a.overlaps(&c));
    }

    #[test]
    fn closer_earns_more() {
        assert_eq!(proximity_share(1.0), 1.0);
        assert!(proximity_share(2.0) > proximity_share(5.0));
        assert_eq!(proximity_share(12.0), 0.0);
    }
}
