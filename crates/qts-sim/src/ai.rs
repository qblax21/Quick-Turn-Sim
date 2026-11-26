// SPDX-License-Identifier: Apache-2.0
//! A computer driver (D-122): it drives through the same input frame a
//! person makes, with the same pedals, the sequential shift buttons and the
//! clutch pedal, and no assists. Run it on a session with automatic clutch,
//! automatic gears and traction control all off ([`AiDriver::session_config`]).
//! It reads only what a driver could sense: where the car is on the track,
//! its speed, slide and rotation, and the engine speed.
//!
//! Two modes:
//! - **Grip:** laps a circuit on a smoothed racing line at a pace given as a
//!   share of an estimated grip limit.
//! - **Drift:** drifts the judged zones of a drift track (D-126). It follows
//!   a line from the outside of each zone toward its inner clipping point,
//!   starts the slide with a pull of the handbrake and power, and holds it
//!   with two loops: the steering puts the front wheels along the front
//!   axle's own travel and asks for the rotation the line and the slide
//!   angle need; the throttle holds the slide angle. Between zones it
//!   drives with grip, easing off as the rears spin up.
//!
//! Like every driver here it is not simulation state: a replay records its
//! input frames.

use crate::drivers::Driver;
use crate::session::{Session, SessionConfig};
use crate::track::{PathGeom, Track};
use qts_core::input::{buttons, InputFrame};
use qts_core::math::{clamp, fm};
use qts_core::step::Simulation;
use qts_vehicle::params::DrivetrainParams;
use qts_vehicle::Assists;

/// How the computer drives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AiMode {
    /// Laps on a racing line.
    Grip,
    /// Drifts the judged zones.
    Drift,
}

/// Settings for the [`AiDriver`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiConfig {
    /// How it drives.
    pub mode: AiMode,
    /// Share of the grip estimate it uses, 0.3 to 1.0 (grip mode), or of
    /// its zone speeds (drift mode).
    pub pace: f64,
    /// Grip it plans corners and braking with at pace 1, in g. An estimate
    /// for road tyres on dry tarmac; the car does not tell it.
    pub grip_g: f64,
    /// Slide it aims for in a zone, degrees (drift mode).
    pub drift_angle_deg: f64,
}

impl AiConfig {
    /// Grip laps at `pace`.
    pub fn grip(pace: f64) -> Self {
        Self {
            mode: AiMode::Grip,
            pace: clamp(pace, 0.3, 1.0),
            grip_g: 0.9,
            drift_angle_deg: 0.0,
        }
    }

    /// Drift laps at `pace`.
    pub fn drift(pace: f64) -> Self {
        Self {
            mode: AiMode::Drift,
            pace: clamp(pace, 0.3, 1.0),
            grip_g: 1.0,
            drift_angle_deg: 20.0,
        }
    }
}

/// Lateral room the line keeps from the edge of the road, m.
const EDGE_MARGIN_M: f64 = 1.6;
/// Spacing of the line's points, m.
const LINE_STEP_M: f64 = 1.0;
/// The slide starts this far before a zone, m.
const ENTRY_LEAD_M: f64 = 22.0;
/// In linked corners the swing to the next corner starts this far
/// before its zone, m.
const LINK_LEAD_M: f64 = 8.0;
/// Shift up above this share of the redline.
const UPSHIFT: f64 = 0.93;
/// Shift down when the next gear down would turn below this share.
const DOWNSHIFT_AFTER: f64 = 0.78;
/// Minimum time between shifts, s.
const SHIFT_GAP_S: f64 = 0.45;
/// Clutch release time when moving off, s.
const LAUNCH_S: f64 = 1.2;
/// Handbrake pull that starts a slide, s.
const HANDBRAKE_S: f64 = 0.3;

/// A judged zone as the driver plans it.
#[derive(Clone, Debug)]
struct Zone {
    from: f64,
    to: f64,
    /// +1 for a left-hand corner, -1 for a right-hander.
    dir: f64,
    /// Speed it carries through, m/s.
    speed: f64,
}

/// The computer driver. See the module notes.
#[derive(Clone, Debug)]
pub struct AiDriver {
    config: AiConfig,
    line: Vec<[f64; 2]>,
    /// Distance along the track path of each line point.
    line_s: Vec<f64>,
    /// Offset of each line point from the middle of the road, m, left
    /// positive.
    offsets: Vec<f64>,
    /// Distance to the left of the line on the last tick, m.
    last_side: f64,
    /// Allowed speed at each line point, from curvature and braking.
    speed: Vec<f64>,
    path_len: f64,
    zones: Vec<Zone>,
    nearest: usize,
    // Pedal and lever state.
    launch_t: Option<f64>,
    last_shift_t: f64,
    prev_buttons: u16,
    pressed_shift: bool,
    prev_slip: f64,
    slip_rate: f64,
    /// Integral of the slide-angle error while holding a slide.
    hold_i: f64,
    /// Signed slide toward the corner a tick ago, degrees.
    prev_bs: f64,
    slide_zone: Option<usize>,
    slide_t: f64,
    cranking: bool,
    last: [f64; 4],
    rpm_per_ms: f64,
    zone_gear_speed: Option<f64>,
    stuck_t: f64,
    /// Times it pressed reset after getting stuck.
    pub resets: u32,
}

impl AiDriver {
    /// The session settings it drives with: no assists of any kind.
    pub fn session_config() -> SessionConfig {
        SessionConfig {
            assists: Assists {
                auto_clutch: false,
                auto_shift: false,
                traction_control: false,
                engine_damage: false,
            },
            ..SessionConfig::default()
        }
    }

    /// A driver for `track`. Drift mode needs a track with judged zones.
    pub fn new(track: &Track, config: AiConfig) -> Result<Self, String> {
        let (path, half_width) = main_path(track)?;
        let room = (half_width - EDGE_MARGIN_M).max(0.5);
        let n = (path.length() / LINE_STEP_M).floor() as usize;
        let line_s: Vec<f64> = (0..n).map(|i| i as f64 * LINE_STEP_M).collect();
        let (offsets, zones) = match config.mode {
            AiMode::Grip => (racing_offsets(path, &line_s, room), Vec::new()),
            AiMode::Drift => drift_offsets(track, path, &line_s, room, config)?,
        };
        let line: Vec<[f64; 2]> = line_s
            .iter()
            .zip(&offsets)
            .map(|(&s, &d)| crate::drift::path_point(path, s, d))
            .collect();
        let mut ai = Self {
            config,
            line,
            line_s,
            offsets,
            last_side: 0.0,
            speed: Vec::new(),
            path_len: path.length(),
            zones,
            nearest: 0,
            launch_t: None,
            last_shift_t: -1.0,
            prev_buttons: 0,
            pressed_shift: false,
            prev_slip: 0.0,
            slip_rate: 0.0,
            hold_i: 0.0,
            prev_bs: 0.0,
            slide_zone: None,
            slide_t: 0.0,
            cranking: false,
            last: [0.0; 4],
            rpm_per_ms: 0.0,
            zone_gear_speed: None,
            stuck_t: 0.0,
            resets: 0,
        };
        ai.plan_speeds();
        Ok(ai)
    }

    /// Finds where on its line the car is, from anywhere on the track (the
    /// tick-by-tick search only looks a little way ahead).
    pub fn sync(&mut self, session: &Session) {
        let st = &session.car().state;
        let (x, y) = (st.pos.x, st.pos.y);
        self.nearest = (0..self.line.len())
            .min_by(|&a, &b| {
                let da = (self.line[a][0] - x).powi(2) + (self.line[a][1] - y).powi(2);
                let db = (self.line[b][0] - x).powi(2) + (self.line[b][1] - y).powi(2);
                da.total_cmp(&db)
            })
            .unwrap_or(0);
    }

    /// Puts the car on its line `ahead_m` past where it is now, pointing
    /// along the line, at rest.
    pub fn place_ahead(&mut self, session: &mut Session, ahead_m: f64) {
        self.sync(session);
        let n = self.line.len();
        let i = (self.nearest + (ahead_m / LINE_STEP_M) as usize) % n;
        let (a, b) = (self.line[i], self.line[(i + 1) % n]);
        let heading = fm::atan2(b[1] - a[1], b[0] - a[0]);
        let ground = session.track().ground_height(a[0], a[1]);
        session.car_mut().place(a[0], a[1], heading, ground);
        self.nearest = i;
    }

    /// The line it follows, world x and y every metre.
    pub fn line(&self) -> &[[f64; 2]] {
        &self.line
    }

    /// Distance along the track path at the car, m.
    pub fn progress_m(&self) -> f64 {
        self.line_s[self.nearest]
    }

    fn g(&self) -> f64 {
        self.config.pace * self.config.grip_g * qts_core::constants::GRAVITY_M_S2
    }

    /// Allowed speed along the line: the corner limit from curvature, then
    /// a backward pass so it can brake for each corner in time.
    fn plan_speeds(&mut self) {
        let n = self.line.len();
        let a_lat = self.g();
        // A drift car has no anti-lock and its rears lock early: it plans
        // gentler braking.
        let a_brake = if self.config.mode == AiMode::Drift {
            0.55 * self.g()
        } else {
            0.85 * self.g()
        };
        let mut v: Vec<f64> = (0..n)
            .map(|i| {
                let k = curvature(&self.line, i).abs();
                if k > 1e-5 {
                    fm::sqrt(a_lat / k).min(70.0)
                } else {
                    70.0
                }
            })
            .collect();
        // Drift zones: the zone's own speed through it and into it.
        for z in &self.zones {
            for (i, s) in self.line_s.iter().enumerate() {
                if *s >= z.from - ENTRY_LEAD_M && *s <= z.to {
                    v[i] = v[i].min(z.speed);
                }
            }
        }
        // Twice round, so the braking for the first corner carries over the
        // end of the lap.
        for _ in 0..2 {
            for k in (0..n).rev() {
                let next = v[(k + 1) % n];
                v[k] = v[k].min(fm::sqrt(next * next + 2.0 * a_brake * LINE_STEP_M));
            }
        }
        self.speed = v;
    }

    fn update_nearest(&mut self, x: f64, y: f64) {
        let n = self.line.len();
        let (mut best, mut best_d2) = (self.nearest, f64::INFINITY);
        for k in 0..90 {
            let i = (self.nearest + n + k - 10) % n;
            let (dx, dy) = (self.line[i][0] - x, self.line[i][1] - y);
            let d2 = dx * dx + dy * dy;
            if d2 < best_d2 {
                best_d2 = d2;
                best = i;
            }
        }
        self.nearest = best;
    }

    fn zone_ahead(&self, s: f64) -> Option<(usize, &Zone)> {
        // Linked corners: the swing to the next one starts a few metres
        // before its zone, so the slide has changed sides by the time the
        // judge looks at it there.
        let linked = self.zones.iter().enumerate().find(|(_, z)| {
            let d = (z.from - s).rem_euclid(self.path_len);
            d <= LINK_LEAD_M
        });
        if let Some((i, z)) = linked {
            let prev = &self.zones[(i + self.zones.len() - 1) % self.zones.len()];
            if (z.from - prev.to).abs() < 1.0 {
                return Some((i, z));
            }
        }
        self.zones.iter().enumerate().find(|(_, z)| {
            let d = (s - (z.from - ENTRY_LEAD_M)).rem_euclid(self.path_len);
            d <= z.to - z.from + ENTRY_LEAD_M
        })
    }

    /// Gear changes on the sequential buttons, from engine speed.
    fn shift(
        &mut self,
        session: &Session,
        t: f64,
        throttle: f64,
        clutch: f64,
        sliding: bool,
    ) -> u16 {
        let car = session.car();
        let drive = &car.output().drive;
        if drive.shifting || t - self.last_shift_t < SHIFT_GAP_S || drive.gear < 1 {
            return 0;
        }
        let DrivetrainParams::EngineManual(m) = &car.params().drivetrain else {
            return 0;
        };
        let redline = m.engine.redline_rpm;
        let ratios = &m.gearbox.ratios;
        let g = drive.gear as usize;
        let rpm = drive.engine_rpm;
        // Sliding, the rear wheels spin faster than the road: hold the gear
        // unless the engine is at the limiter.
        let moving = clutch < 0.1 && car.output().speed > 4.0;
        // Engine speed per m/s of road speed in each gear's ratio: from the
        // final drive and the rolling radius of a front wheel, which is not
        // driven, so wheelspin does not fool it.
        let speed = car.output().speed;
        let radius = car.output().wheels[0].rolling_radius.max(0.1);
        self.rpm_per_ms = m.gearbox.final_drive * 60.0 / (std::f64::consts::TAU * radius);
        let road_rpm = self.rpm_per_ms * ratios[g - 1] * speed;
        // Sliding, the gear stays where it is (a shift unsettles the rear)
        // unless the engine is on the limiter, which cuts the power.
        if sliding {
            // Spinning rears put the engine on the limiter, which the
            // throttle sorts out; shift only when the road speed itself is
            // too much for the gear.
            if road_rpm > 0.85 * redline && clutch < 0.1 && g < ratios.len() {
                self.last_shift_t = t;
                return buttons::SHIFT_UP;
            }
            return 0;
        }
        // Approaching a zone: the gear that puts its speed at 70 percent of
        // the redline.
        if let Some(zone_speed) = self.zone_gear_speed {
            if self.rpm_per_ms > 0.0 && speed > 5.0 {
                let fit = (1..=ratios.len())
                    .find(|&k| {
                        self.rpm_per_ms * ratios[k - 1] * zone_speed.min(speed.max(8.0))
                            < 0.7 * redline
                    })
                    .unwrap_or(ratios.len())
                    // Never first for a drift: the slightest throttle spins
                    // the rears there.
                    .max(2.min(ratios.len()));
                let after = self.rpm_per_ms * ratios[g.saturating_sub(2)] * speed;
                if fit < g
                    && g > 1
                    && after < 0.75 * redline
                    && (throttle < 0.5 || rpm < 0.6 * redline)
                {
                    self.last_shift_t = t;
                    return buttons::SHIFT_DOWN;
                }
                if fit > g {
                    self.last_shift_t = t;
                    return buttons::SHIFT_UP;
                }
                return 0;
            }
        }
        if moving
            && rpm > UPSHIFT * redline
            && road_rpm > 0.8 * redline
            && throttle > 0.3
            && g < ratios.len()
        {
            {
                self.last_shift_t = t;
                return buttons::SHIFT_UP;
            }
        }
        if g > 1 {
            let after = rpm * ratios[g - 2] / ratios[g - 1];
            if after < DOWNSHIFT_AFTER * redline && rpm < 0.5 * redline {
                self.last_shift_t = t;
                return buttons::SHIFT_DOWN;
            }
        }
        0
    }
}

/// The path the track is driven on and the half width of its road.
fn main_path(track: &Track) -> Result<(&PathGeom, f64), String> {
    let def = track.def();
    let (id, half) = match (&def.drift, &def.track_limits) {
        (Some(d), Some(l)) => (d.path.clone(), l.half_width_m),
        (None, Some(l)) => (l.path.clone(), l.half_width_m),
        (Some(d), None) => (d.path.clone(), 6.0),
        (None, None) => (
            track
                .paths()
                .first()
                .map(|p| p.id.clone())
                .ok_or("the track has no path to drive")?,
            5.0,
        ),
    };
    let path = track
        .path(&id)
        .ok_or_else(|| format!("the track has no path `{id}`"))?;
    // Track limits reach to the kerbs; the line keeps to the road.
    Ok((path, (half - 1.2).max(2.0)))
}

/// Signed curvature at point `i` of a closed line, 1/m.
fn curvature(p: &[[f64; 2]], i: usize) -> f64 {
    let n = p.len();
    let (a, b, c) = (p[(i + n - 3) % n], p[i], p[(i + 3) % n]);
    let (ax, ay) = (b[0] - a[0], b[1] - a[1]);
    let (bx, by) = (c[0] - b[0], c[1] - b[1]);
    let la = fm::sqrt(ax * ax + ay * ay);
    let lb = fm::sqrt(bx * bx + by * by);
    if la < 1e-9 || lb < 1e-9 {
        return 0.0;
    }
    fm::atan2(ax * by - ay * bx, ax * bx + ay * by) / (0.5 * (la + lb))
}

/// A racing line as offsets across the path: an elastic band pulled tight
/// between the road edges, which straightens the corners (outside, apex,
/// outside) without a model of the car.
fn racing_offsets(path: &PathGeom, line_s: &[f64], room: f64) -> Vec<f64> {
    let n = line_s.len();
    let base: Vec<([f64; 2], f64)> = line_s.iter().map(|&s| path.at(s)).collect();
    let mut d = vec![0.0; n];
    for _ in 0..1500 {
        let pts: Vec<[f64; 2]> = (0..n)
            .map(|i| {
                let (p, h) = base[i];
                [p[0] - d[i] * fm::sin(h), p[1] + d[i] * fm::cos(h)]
            })
            .collect();
        for i in 0..n {
            let (a, c) = (pts[(i + n - 1) % n], pts[(i + 1) % n]);
            let mid = [0.5 * (a[0] + c[0]), 0.5 * (a[1] + c[1])];
            let (_, h) = base[i];
            // Move along the path's normal toward the neighbours' midpoint.
            let lateral = (mid[0] - pts[i][0]) * -fm::sin(h) + (mid[1] - pts[i][1]) * fm::cos(h);
            d[i] = clamp(d[i] + 0.5 * lateral, -room, room);
        }
    }
    d
}

/// Offsets for drifting: through each zone, from its outside at the entry
/// to the inner clipping point, then back to the middle; the middle of the
/// road elsewhere. Smoothed so the line has no kinks.
fn drift_offsets(
    track: &Track,
    path: &PathGeom,
    line_s: &[f64],
    room: f64,
    config: AiConfig,
) -> Result<(Vec<f64>, Vec<Zone>), String> {
    let def = track
        .def()
        .drift
        .as_ref()
        .ok_or("drift mode needs a track with judged zones")?;
    let mut keys: Vec<(f64, f64)> = Vec::new();
    let mut zones = Vec::new();
    for z in &def.zones {
        let dir = z.icp_offset_m.signum();
        let outside = -dir * 0.4 * room;
        let inside = dir * (room - 2.0);
        keys.push((z.from_m - ENTRY_LEAD_M, outside * 0.7));
        keys.push((z.from_m, outside));
        keys.push((z.icp_at_m, inside));
        keys.push((z.to_m, 0.3 * inside));
        // Radius of the zone from the path's turn through it.
        let (_, h0) = path.at(z.from_m);
        let (_, h1) = path.at(z.to_m);
        let turn = (h1 - h0).abs().max(0.2);
        let radius = (z.to_m - z.from_m) / turn;
        // Zone speed for about 0.35 g of cornering: well under grip driving,
        // which leaves room for the slide to wander.
        let speed = (fm::sqrt(0.35 * 9.81 * radius) * config.pace).clamp(8.0, 22.0);
        zones.push(Zone {
            from: z.from_m,
            to: z.to_m,
            dir,
            speed,
        });
    }
    keys.sort_by(|a, b| a.0.total_cmp(&b.0));
    let len = path.length();
    let raw: Vec<f64> = line_s
        .iter()
        .map(|&s| interpolate_closed(&keys, s, len))
        .collect();
    // Moving average over 15 m either side.
    let n = raw.len();
    let w = 15usize;
    let smooth = (0..n)
        .map(|i| (0..=2 * w).map(|k| raw[(i + n + k - w) % n]).sum::<f64>() / (2 * w + 1) as f64)
        .collect();
    Ok((smooth, zones))
}

/// Linear interpolation between `(s, value)` keys on a closed loop of
/// length `len`; 0 well away from any key.
fn interpolate_closed(keys: &[(f64, f64)], s: f64, len: f64) -> f64 {
    if keys.is_empty() {
        return 0.0;
    }
    // Neighbouring keys, with a return to 0 across gaps longer than 60 m.
    let next = keys.iter().position(|k| k.0 > s);
    let (a, b) = match next {
        Some(0) => (
            (keys[keys.len() - 1].0 - len, keys[keys.len() - 1].1),
            keys[0],
        ),
        Some(j) => (keys[j - 1], keys[j]),
        None => (keys[keys.len() - 1], (keys[0].0 + len, keys[0].1)),
    };
    let gap = b.0 - a.0;
    if gap > 60.0 {
        // Out to the middle and back.
        if s - a.0 < 25.0 {
            return a.1 * (1.0 - (s - a.0) / 25.0);
        }
        if b.0 - s < 25.0 {
            return b.1 * (1.0 - (b.0 - s) / 25.0);
        }
        return 0.0;
    }
    a.1 + (b.1 - a.1) * (s - a.0) / gap.max(1e-9)
}

impl Driver for AiDriver {
    fn frame(&mut self, session: &Session) -> InputFrame {
        let t = session.tick() as f64 / f64::from(session.tick_rate_hz());
        let car = session.car();
        let st = &car.state;
        let out = car.output();
        let p = car.params();
        let (x, y) = (st.pos.x, st.pos.y);
        let yaw = st.rot.yaw();
        let v = out.vel_body;
        let speed = fm::sqrt(v[0] * v[0] + v[1] * v[1]);
        let slip_deg = fm::atan2(v[1], v[0].abs().max(0.5)).to_degrees();
        let yaw_rate = out.yaw_rate;
        self.update_nearest(x, y);
        let n = self.line.len();
        let s = self.line_s[self.nearest];
        let max_road = p.steering.max_wheel_angle_rad / p.steering.ratio;
        let wheel = |road_rad: f64| clamp(road_rad / max_road, -1.0, 1.0);

        // Pure pursuit on the line: aim at a point ahead and steer the
        // velocity (not the nose) at it, so the same law works sliding.
        let look = (4.0 + 0.45 * speed).min(30.0);
        let target = self.line[(self.nearest + look as usize) % n];
        let (tx, ty) = (target[0] - x, target[1] - y);
        let course = yaw + fm::atan2(v[1], v[0].max(0.5));
        let err = wrap(fm::atan2(ty, tx) - course);
        let dist = fm::sqrt(tx * tx + ty * ty).max(1.0);
        // Plus a correction for how far the car is to the side of the line,
        // which pure pursuit alone leaves standing in a long corner.
        let here = self.line[self.nearest];
        let next = self.line[(self.nearest + 1) % n];
        let (hx, hy) = (next[0] - here[0], next[1] - here[1]);
        let hl = fm::sqrt(hx * hx + hy * hy).max(1e-9);
        let side = (hx * (y - here[1]) - hy * (x - here[0])) / hl;
        self.last_side = side;
        let pursuit = fm::atan2(2.0 * p.body.wheelbase_m * fm::sin(err), dist)
            - clamp(0.6 * side / (speed + 5.0), -0.08, 0.08);

        // Speed held by the pedals: planned speed a little way ahead.
        let ahead = (self.nearest + (speed * 0.3) as usize) % n;
        // Running wide of the line means more speed than grip: ease off.
        let want = self.speed[ahead] - 2.0 * (side.abs() - 1.0).max(0.0);
        let error = want - speed;
        let (mut throttle, mut brake) = if error >= 0.0 {
            (clamp(0.25 + 0.35 * error, 0.0, 1.0), 0.0)
        } else {
            (0.0, clamp(-0.25 * error, 0.0, 1.0))
        };
        if self.config.mode == AiMode::Drift {
            brake = brake.min(0.45);
        }
        let mut steer = wheel(pursuit);
        let mut clutch: f64 = 0.0;
        let mut handbrake = 0.0;
        let mut sliding = false;

        let abs_slip = slip_deg.abs();
        let slip_rate = (abs_slip - self.prev_slip) * f64::from(session.tick_rate_hz());
        self.prev_slip = abs_slip;
        self.slip_rate += 0.02 * (slip_rate - self.slip_rate);
        if self.config.mode == AiMode::Drift {
            if let Some((zi, z)) = self.zone_ahead(s).map(|(i, z)| (i, z.clone())) {
                self.zone_gear_speed = Some(z.speed);
                let into = (s - (z.from - ENTRY_LEAD_M)).rem_euclid(self.path_len);
                let want_slip = -z.dir * self.config.drift_angle_deg;
                if self.slide_zone != Some(zi) && (into > 0.6 * ENTRY_LEAD_M || abs_slip > 10.0) {
                    self.slide_zone = Some(zi);
                    self.slide_t = t;
                }
                if self.slide_zone == Some(zi) && (speed > 7.0 || (abs_slip > 8.0 && speed > 2.5)) {
                    sliding = true;
                    // Slide toward the corner, degrees: positive the right
                    // way round, negative the wrong way.
                    let sgn = want_slip.signum();
                    let bs = sgn * slip_deg;
                    let bs_rate = (bs - self.prev_bs) * f64::from(session.tick_rate_hz());
                    // Rotation the line needs: pure pursuit's curvature
                    // (which also closes on the line) times speed.
                    let target = want_slip.abs();
                    // Plus the rotation that turns the body to the slide
                    // angle wanted: the velocity turns at the line's rate,
                    // the body that much faster while the angle grows.
                    let want_yaw = 2.0 * speed * fm::sin(err) / dist
                        - sgn * 1.0 * clamp(target - bs, -15.0, 15.0).to_radians();
                    // Steering: the front wheels along the front axle's own
                    // direction of travel, so they carry no force, and then
                    // a front slip angle that asks for the rotation the line
                    // needs. Opposite lock follows the slide by itself.
                    let a = p.body.cg_to_front_axle_m;
                    let beta_f = fm::atan2(v[1] + a * yaw_rate, v[0].abs().max(0.5));
                    let alpha = clamp(0.35 * (want_yaw - yaw_rate), -0.2, 0.2);
                    steer = wheel(beta_f + alpha);
                    let entering = bs < 8.0 && bs > -4.0;
                    if entering && t - self.slide_t > 1.5 && speed > 6.0 && bs < 4.0 {
                        self.slide_t = t;
                        self.hold_i = 0.0;
                    }
                    throttle = if bs <= -4.0 {
                        // Swinging across to the other side (a transition):
                        // the steering turns it; part throttle keeps the
                        // rear light.
                        0.25
                    } else if entering && t - self.slide_t < 1.2 {
                        // Breaking the rear loose: a pull of the handbrake
                        // with the clutch in and the wheel turned in a
                        // little, then power with the holding steering.
                        if t - self.slide_t < HANDBRAKE_S {
                            steer = wheel(pursuit + z.dir * 0.12);
                            handbrake = 1.0;
                            clutch = 1.0;
                            0.35
                        } else {
                            0.9
                        }
                    } else {
                        // Holding: throttle sets the slide, with an integral
                        // for the throttle a steady slide needs and damping
                        // on how fast it grows.
                        let e = target - bs;
                        self.hold_i = clamp(
                            self.hold_i + 0.004 * e / f64::from(session.tick_rate_hz()) * 10.0,
                            -0.3,
                            0.3,
                        );
                        let thr = clamp(
                            0.45 + 0.025 * e + self.hold_i - 0.006 * bs_rate.clamp(-80.0, 80.0),
                            0.0,
                            1.0,
                        );
                        // Well past the angle: off the power, which gives
                        // the rear its grip back.
                        if e < -10.0 {
                            thr.min(0.15)
                        } else {
                            thr
                        }
                    };
                    self.prev_bs = bs;
                    // Speed: left foot on the brake when well over the zone's.
                    let fast = speed - z.speed - 1.0;
                    brake = if entering {
                        0.0
                    } else {
                        clamp(0.1 * fast, 0.0, 0.4)
                    };
                } else {
                    // Turn in toward the corner.
                    steer = wheel(pursuit + z.dir * 0.05);
                }
            } else {
                self.slide_zone = None;
                self.zone_gear_speed = self
                    .zones
                    .iter()
                    .find(|z| (z.from - ENTRY_LEAD_M - s).rem_euclid(self.path_len) < 80.0)
                    .map(|z| z.speed);
                // Between zones a slide left over is caught, not driven.
                if abs_slip > 6.0 {
                    // The same steering as holding a slide, with no angle
                    // wanted: the front wheels along their travel, and the
                    // rotation that brings the body back in line.
                    let a = p.body.cg_to_front_axle_m;
                    let beta_f = fm::atan2(v[1] + a * yaw_rate, v[0].abs().max(0.5));
                    let want_yaw = 2.0 * speed * fm::sin(err) / dist
                        + 1.0 * clamp(slip_deg, -15.0, 15.0).to_radians();
                    let alpha = clamp(0.35 * (want_yaw - yaw_rate), -0.2, 0.2);
                    steer = wheel(beta_f + alpha);
                    throttle = if abs_slip > 20.0 {
                        0.0
                    } else {
                        throttle.min(0.3)
                    };
                    // Braking hard with the tail out spins it.
                    brake = brake.min(0.15);
                }
            }
        }
        // Away from a slide, ease off as the rears spin up, as a driver does
        // by feel: past about 10 percent slip they give up grip sideways.
        if !sliding && self.config.mode == AiMode::Drift {
            let spin = out.wheels[2].slip_ratio.max(out.wheels[3].slip_ratio);
            throttle *= clamp(1.0 - 4.0 * (spin - 0.1), 0.25, 1.0);
        }
        // Moving off: first gear on the clutch, then let it out.
        let drive = &out.drive;
        let mut press = 0u16;
        if drive.gear < 1 {
            clutch = 1.0;
            throttle = 0.0;
            if !self.pressed_shift && t - self.last_shift_t > SHIFT_GAP_S {
                press = buttons::SHIFT_UP;
                self.last_shift_t = t;
            }
            self.launch_t = None;
        } else if speed < 3.0 && self.launch_t.is_none() && drive.gear == 1 {
            self.launch_t = Some(t);
        }
        if let Some(t0) = self.launch_t {
            let k = (t - t0) / LAUNCH_S;
            if k >= 1.0 || speed > 6.0 {
                self.launch_t = None;
            } else {
                clutch = clutch.max(1.0 - k);
                throttle = throttle.max(0.45);
                brake = 0.0;
            }
        }
        // Nearly stopped in a high gear: clutch down, off the throttle, and
        // down through the box to first.
        let mut force_down = false;
        if speed < 3.0 && drive.gear > 1 {
            clutch = 1.0;
            throttle = 0.0;
            force_down = !drive.shifting && t - self.last_shift_t > SHIFT_GAP_S;
        }
        // Stuck (beached or spun facing a wall): the reset a player would use.
        if speed < 2.0 {
            self.stuck_t += 1.0 / f64::from(session.tick_rate_hz());
        } else {
            self.stuck_t = 0.0;
        }
        // Never stall: dip the clutch when the engine runs down.
        if let DrivetrainParams::EngineManual(m) = &p.drivetrain {
            // At any speed: rear wheels locking under braking take the
            // engine down with them.
            if drive.engine_rpm < 1.15 * m.engine.idle_rpm && drive.gear >= 1 {
                clutch = clutch.max(0.8);
            }
        }
        if force_down {
            press = buttons::SHIFT_DOWN;
            self.last_shift_t = t;
        }
        if press == 0 {
            press = self.shift(session, t, throttle, clutch, sliding || abs_slip > 6.0);
        }
        if self.stuck_t > 5.0 {
            press = buttons::RESET;
            self.resets += 1;
            self.stuck_t = 0.0;
            self.nearest = 0;
        }
        // A stalled engine: clutch down, out of gear and the starter.
        let idle = match &p.drivetrain {
            DrivetrainParams::EngineManual(m) => m.engine.idle_rpm,
            _ => 0.0,
        };
        if drive.engine_rpm < 300.0 && speed < 2.0 && idle > 0.0 {
            self.cranking = true;
        }
        if self.cranking && drive.engine_rpm > 0.9 * idle {
            self.cranking = false;
        }
        if self.cranking {
            clutch = 1.0;
            throttle = 0.0;
            press = buttons::STARTER;
            self.launch_t = None;
        }
        // Buttons act on the rising edge: release for a tick between presses.
        let shifts = buttons::SHIFT_UP | buttons::SHIFT_DOWN | buttons::RESET;
        let held = (press & shifts & !self.prev_buttons) | (press & buttons::STARTER);
        if held & shifts != 0 && std::env::var("QTS_AI_TRACE").ok().as_deref() == Some("shifts") {
            eprintln!(
                "t {t:6.2} s {s:6.0} gear {} rpm {:5.0} v {speed:4.1} press {held} sliding {sliding} zone {:?} k {:.1} clutch {clutch:.2} thr {throttle:.2}",
                drive.gear, drive.engine_rpm, self.zone_gear_speed, self.rpm_per_ms
            );
        }
        self.prev_buttons = held;
        self.pressed_shift = held != 0;
        self.last = [steer, throttle, clutch, handbrake];
        let mut f = InputFrame::from_analog(steer, throttle, brake, clutch, handbrake);
        f.buttons = held;
        f
    }
}

fn wrap(a: f64) -> f64 {
    fm::atan2(fm::sin(a), fm::cos(a))
}

/// What [`run_laps`] saw.
#[derive(Clone, Debug, Default)]
pub struct AiRun {
    /// Each finished lap: time in s, whether it counted, and the drift
    /// score on a track with judged zones.
    pub laps: Vec<(f64, bool, Option<f64>)>,
    /// The coach's note on the last drift lap.
    pub coach: Option<String>,
    /// Corner scores of the last drift lap: name, score, why.
    pub zones: Vec<(String, f64, String)>,
    /// Corner scores of every judged lap, in order.
    pub lap_zones: Vec<Vec<(String, f64, String)>>,
    /// Simulated time driven, s.
    pub seconds: f64,
}

/// Drives `laps` timed laps (or until `max_seconds`) with `ai`, judging
/// drift zones where the track has them.
pub fn run_laps(session: &mut Session, ai: &mut AiDriver, laps: u32, max_seconds: f64) -> AiRun {
    run_laps_recorded(session, ai, laps, max_seconds, None)
}

/// [`run_laps`], recording every input frame into `recorder` for a replay.
pub fn run_laps_recorded(
    session: &mut Session,
    ai: &mut AiDriver,
    laps: u32,
    max_seconds: f64,
    mut recorder: Option<&mut qts_core::replay::ReplayRecorder>,
) -> AiRun {
    let trace_every: u64 = std::env::var("QTS_AI_TRACE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);
    let zone_trace = std::env::var("QTS_AI_TRACE").ok().as_deref() == Some("zones");
    let trace = !zone_trace && std::env::var_os("QTS_AI_TRACE").is_some();
    let mut cur_zone: Option<usize> = None;
    let mut acc = [0.0_f64; 8];
    let mut judge = crate::drift::DriftJudge::new(session);
    let slot = judge.slot();
    let mut run = AiRun::default();
    let mut seen = (session.timing().runs, session.timing().invalid_runs);
    let mut serial = 0;
    let rate = f64::from(session.tick_rate_hz());
    let start = session.tick();
    while (run.laps.len() as u32) < laps && ((session.tick() - start) as f64 / rate) < max_seconds {
        let frame = ai.frame(session);
        let started = session.timing().start_tick;
        session.step(&frame);
        if let Some(rec) = recorder.as_deref_mut() {
            rec.record(frame, session);
        }
        judge.observe(session);
        if zone_trace {
            let z = ai.zone_ahead(ai.progress_m()).map(|(i, _)| i);
            let o = session.car().output();
            let slip = fm::atan2(o.vel_body[1], o.vel_body[0].abs().max(0.5)).to_degrees();
            let w = ai.line[ai.nearest];
            let st = &session.car().state;
            let off = fm::sqrt((st.pos.x - w[0]).powi(2) + (st.pos.y - w[1]).powi(2));
            if z != cur_zone {
                if let Some(i) = cur_zone {
                    eprintln!(
                        "zone {i}: slip max {:5.1} mean {:5.1}  off max {:4.1}  road {:5.1}..{:5.1}  speed {:4.1}..{:4.1}  resets {}",
                        acc[0], acc[1] / acc[2].max(1.0), acc[3], acc[6], acc[7], acc[4], acc[5], ai.resets
                    );
                }
                cur_zone = z;
                acc = [0.0, 0.0, 0.0, 0.0, f64::MAX, 0.0, f64::MAX, f64::MIN];
            }
            if cur_zone.is_some() {
                acc[0] = acc[0].max(slip.abs());
                acc[1] += slip.abs();
                acc[2] += 1.0;
                acc[3] = acc[3].max(off);
                acc[4] = acc[4].min(o.speed);
                acc[5] = acc[5].max(o.speed);
                let road = ai.offsets[ai.nearest] + ai.last_side;
                acc[6] = acc[6].min(road);
                acc[7] = acc[7].max(road);
            }
        }
        if trace && session.tick() % trace_every == 0 {
            let o = session.car().output();
            let tm = session.timing();
            let w = ai.line[ai.nearest];
            let st = &session.car().state;
            let off = fm::sqrt((st.pos.x - w[0]).powi(2) + (st.pos.y - w[1]).powi(2));
            eprintln!(
                "t {:6.1} s {:7.1} v {:5.1}/{:5.1} g {} rpm {:5.0} slip {:5.1} off {:4.1} cp {} broken {} yaw {:5.2} in {:5.2} {:4.2} {:3.1} {:3.1}",
                session.tick() as f64 / rate, ai.progress_m(), o.speed, ai.speed[ai.nearest],
                o.drive.gear, o.drive.engine_rpm,
                fm::atan2(o.vel_body[1], o.vel_body[0].abs().max(0.5)).to_degrees(),
                off, tm.checkpoints_passed, tm.limits_broken, o.yaw_rate, ai.last[0], ai.last[1], ai.last[2], ai.last[3]
            );
        }
        let tm = session.timing();
        if (tm.runs, tm.invalid_runs) != seen {
            let counted = tm.runs > seen.0;
            let ticks = session.tick() - started;
            seen = (tm.runs, tm.invalid_runs);
            run.laps.push((ticks as f64 / rate, counted, None));
        }
        let s = slot.lock().unwrap_or_else(|p| p.into_inner());
        if s.serial != serial {
            serial = s.serial;
            if let (Some(lap), Some(last)) = (&s.last_lap, run.laps.last_mut()) {
                last.2 = Some(lap.total);
                run.zones = lap
                    .zones
                    .iter()
                    .zip(
                        session
                            .track()
                            .def()
                            .drift
                            .iter()
                            .flat_map(|d| d.zones.iter()),
                    )
                    .map(|(z, d)| match z {
                        Some(z) => (d.name.clone(), z.total, z.describe()),
                        None => (d.name.clone(), 0.0, "not judged".to_string()),
                    })
                    .collect();
                run.lap_zones.push(run.zones.clone());
            }
            run.coach = s.coach.clone();
        }
    }
    run.seconds = (session.tick() - start) as f64 / rate;
    run
}
