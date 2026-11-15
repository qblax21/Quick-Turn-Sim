// SPDX-License-Identifier: Apache-2.0
//! Scripted drivers for headless runs, soak tests and validation.
//!
//! A driver looks at the session and produces the input frame for the next
//! tick. Drivers are not part of the simulation state: what they produce is
//! recorded as input frames, and a replay feeds those frames back without
//! running the driver again.

use crate::session::Session;
use crate::track::PathGeom;
use qts_core::input::{buttons, InputFrame};
use qts_core::math::{clamp, fm};
use qts_core::step::Simulation;

/// Something that produces input frames.
pub trait Driver {
    /// The input for the session's next tick.
    fn frame(&mut self, session: &Session) -> InputFrame;
}

/// A driver that touches nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Idle;

impl Driver for Idle {
    fn frame(&mut self, _session: &Session) -> InputFrame {
        InputFrame::default()
    }
}

/// A driver that plays back a fixed list of `(duration, frame)` steps, then
/// holds the last one.
#[derive(Clone, Debug)]
pub struct Scripted {
    steps: Vec<(u64, InputFrame)>,
    start_tick: Option<u64>,
}

impl Scripted {
    /// Builds a script from `(seconds, frame)` steps for a session running
    /// at `tick_rate_hz`.
    pub fn new(tick_rate_hz: u32, steps: &[(f64, InputFrame)]) -> Self {
        Self {
            steps: steps
                .iter()
                .map(|(seconds, frame)| {
                    ((seconds * f64::from(tick_rate_hz)).round() as u64, *frame)
                })
                .collect(),
            start_tick: None,
        }
    }
}

impl Driver for Scripted {
    fn frame(&mut self, session: &Session) -> InputFrame {
        let start = *self.start_tick.get_or_insert(session.tick());
        let mut elapsed = session.tick() - start;
        for (ticks, frame) in &self.steps {
            if elapsed < *ticks {
                return *frame;
            }
            elapsed -= ticks;
        }
        self.steps.last().map(|s| s.1).unwrap_or_default()
    }
}

/// Small deterministic pseudo-random generator (xorshift64*). Used only by
/// drivers, never inside the simulation step.
#[derive(Clone, Copy, Debug)]
pub struct Rng(u64);

impl Rng {
    /// Creates a generator from a seed.
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform value in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// Uniform value in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// A driver that makes erratic but physically possible inputs: it holds a
/// random target for a random time, slewing the controls toward it, with
/// occasional handbrake pulls, gear changes and resets. Used by the soak
/// test to push the model into corners a sane driver avoids.
#[derive(Clone, Debug)]
pub struct Erratic {
    rng: Rng,
    hold_ticks: u32,
    steer_target: f64,
    throttle_target: f64,
    brake_target: f64,
    handbrake: f64,
    steer: f64,
    throttle: f64,
    brake: f64,
    buttons: u16,
    button_ticks: u32,
}

impl Erratic {
    /// Creates the driver from a seed.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            hold_ticks: 0,
            steer_target: 0.0,
            throttle_target: 0.0,
            brake_target: 0.0,
            handbrake: 0.0,
            steer: 0.0,
            throttle: 0.0,
            brake: 0.0,
            buttons: 0,
            button_ticks: 0,
        }
    }
}

impl Driver for Erratic {
    fn frame(&mut self, session: &Session) -> InputFrame {
        let rate = f64::from(session.tick_rate_hz());
        if self.hold_ticks == 0 {
            self.hold_ticks = (self.rng.range(0.2, 3.0) * rate) as u32;
            // Cubing biases steering toward small angles with rare full lock.
            let s = self.rng.range(-1.0, 1.0);
            self.steer_target = s * s * s;
            let mode = self.rng.unit();
            (self.throttle_target, self.brake_target) = if mode < 0.55 {
                (self.rng.range(0.3, 1.0), 0.0)
            } else if mode < 0.8 {
                (0.0, self.rng.range(0.2, 1.0))
            } else if mode < 0.9 {
                (self.rng.range(0.2, 1.0), self.rng.range(0.2, 1.0))
            } else {
                (0.0, 0.0)
            };
            self.handbrake = if self.rng.unit() < 0.06 { 1.0 } else { 0.0 };
            let pick = self.rng.unit();
            self.buttons = if pick < 0.10 {
                buttons::SHIFT_UP
            } else if pick < 0.18 {
                buttons::SHIFT_DOWN
            } else if pick < 0.185 {
                buttons::RESET
            } else {
                0
            };
            self.button_ticks = 20;
        }
        self.hold_ticks -= 1;
        let slew = |current: f64, target: f64, per_second: f64| {
            let step = per_second / rate;
            current + clamp(target - current, -step, step)
        };
        self.steer = slew(self.steer, self.steer_target, 4.0);
        self.throttle = slew(self.throttle, self.throttle_target, 8.0);
        self.brake = slew(self.brake, self.brake_target, 10.0);
        let mut frame =
            InputFrame::from_analog(self.steer, self.throttle, self.brake, 0.0, self.handbrake);
        if self.button_ticks > 0 {
            self.button_ticks -= 1;
            frame.buttons = self.buttons;
        }
        frame
    }
}

/// Tuning of the [`PathFollower`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathFollowerConfig {
    /// Speed ceiling, m/s.
    pub max_speed: f64,
    /// Lateral acceleration the driver plans corners for, m/s².
    pub lateral_accel: f64,
    /// Deceleration the driver plans braking with, m/s².
    pub brake_decel: f64,
    /// Look-ahead distance at standstill, m.
    pub lookahead_base: f64,
    /// Extra look-ahead per m/s of speed, s.
    pub lookahead_gain: f64,
    /// Throttle per m/s of speed deficit.
    pub throttle_gain: f64,
    /// Brake per m/s of speed excess.
    pub brake_gain: f64,
    /// Treat the path as a closed loop.
    pub closed: bool,
}

impl Default for PathFollowerConfig {
    fn default() -> Self {
        Self {
            max_speed: 30.0,
            lateral_accel: 5.0,
            brake_decel: 4.0,
            lookahead_base: 3.0,
            lookahead_gain: 0.3,
            throttle_gain: 0.5,
            brake_gain: 0.4,
            closed: false,
        }
    }
}

/// A pure-pursuit driver: steers at a point ahead on a path and holds a
/// speed that the upcoming curvature allows. It expects the session to run
/// with automatic clutch and gear selection.
#[derive(Clone, Debug)]
pub struct PathFollower {
    points: Vec<[f64; 2]>,
    s: Vec<f64>,
    curvature: Vec<f64>,
    length: f64,
    config: PathFollowerConfig,
    nearest: usize,
}

impl PathFollower {
    /// Follows a track path.
    pub fn new(path: &PathGeom, config: PathFollowerConfig) -> Self {
        Self::from_points(path.points.clone(), config)
    }

    /// Follows a circle of the given radius, counter-clockwise, starting at
    /// the point of the circle nearest `(start_x, start_y)`.
    pub fn circle(
        center: [f64; 2],
        radius: f64,
        start: [f64; 2],
        mut config: PathFollowerConfig,
    ) -> Self {
        config.closed = true;
        let n = ((2.0 * std::f64::consts::PI * radius) / 0.5)
            .ceil()
            .max(64.0) as usize;
        let phase = fm::atan2(start[1] - center[1], start[0] - center[0]);
        let points = (0..n)
            .map(|i| {
                let a = phase + 2.0 * std::f64::consts::PI * i as f64 / n as f64;
                [
                    center[0] + radius * fm::cos(a),
                    center[1] + radius * fm::sin(a),
                ]
            })
            .collect();
        Self::from_points(points, config)
    }

    fn from_points(points: Vec<[f64; 2]>, config: PathFollowerConfig) -> Self {
        let n = points.len();
        let mut s = vec![0.0; n];
        for i in 1..n {
            let (dx, dy) = (
                points[i][0] - points[i - 1][0],
                points[i][1] - points[i - 1][1],
            );
            s[i] = s[i - 1] + fm::sqrt(dx * dx + dy * dy);
        }
        let mut length = s[n - 1];
        if config.closed {
            let (dx, dy) = (
                points[0][0] - points[n - 1][0],
                points[0][1] - points[n - 1][1],
            );
            length += fm::sqrt(dx * dx + dy * dy);
        }
        // Curvature from the turn between neighbouring segments.
        let mut curvature = vec![0.0; n];
        for i in 0..n {
            let (prev, next) = if config.closed {
                ((i + n - 1) % n, (i + 1) % n)
            } else if i == 0 || i == n - 1 {
                continue;
            } else {
                (i - 1, i + 1)
            };
            let (ax, ay) = (
                points[i][0] - points[prev][0],
                points[i][1] - points[prev][1],
            );
            let (bx, by) = (
                points[next][0] - points[i][0],
                points[next][1] - points[i][1],
            );
            let la = fm::sqrt(ax * ax + ay * ay);
            let lb = fm::sqrt(bx * bx + by * by);
            if la > 1e-9 && lb > 1e-9 {
                let turn = fm::atan2(ax * by - ay * bx, ax * bx + ay * by);
                curvature[i] = turn / (0.5 * (la + lb));
            }
        }
        Self {
            points,
            s,
            curvature,
            length,
            config,
            nearest: 0,
        }
    }

    /// Changes the speed ceiling while driving.
    pub fn set_max_speed(&mut self, max_speed: f64) {
        self.config.max_speed = max_speed;
    }

    /// Distance along the path of the point nearest the car at the last
    /// call to [`Driver::frame`].
    pub fn progress(&self) -> f64 {
        self.s[self.nearest]
    }

    /// True once an open path has been driven to its end.
    pub fn finished(&self) -> bool {
        !self.config.closed && self.nearest + 2 >= self.points.len()
    }

    fn index(&self, i: usize) -> usize {
        if self.config.closed {
            i % self.points.len()
        } else {
            i.min(self.points.len() - 1)
        }
    }

    fn update_nearest(&mut self, x: f64, y: f64) {
        let n = self.points.len();
        let mut best = self.nearest;
        let mut best_d2 = f64::INFINITY;
        // Search a window around the previous nearest point so a path that
        // passes close to itself cannot capture the driver.
        let back = 5usize;
        let ahead = 60usize;
        for k in 0..(back + ahead) {
            let raw = self.nearest + n + k - back;
            let i = if self.config.closed {
                raw % n
            } else {
                match (self.nearest + k).checked_sub(back) {
                    Some(v) if v < n => v,
                    _ => continue,
                }
            };
            let (dx, dy) = (self.points[i][0] - x, self.points[i][1] - y);
            let d2 = dx * dx + dy * dy;
            if d2 < best_d2 {
                best_d2 = d2;
                best = i;
            }
        }
        self.nearest = best;
    }
}

impl Driver for PathFollower {
    fn frame(&mut self, session: &Session) -> InputFrame {
        let car = session.car();
        let state = &car.state;
        let (x, y) = (state.pos.x, state.pos.y);
        let yaw = state.rot.yaw();
        let speed = car.output().vel_body[0].max(0.0);
        self.update_nearest(x, y);
        let cfg = self.config;
        let n = self.points.len();

        // Steering: aim at a point a look-ahead distance along the path.
        let lookahead = cfg.lookahead_base + cfg.lookahead_gain * speed;
        let mut target = self.nearest;
        let mut travelled = 0.0;
        while travelled < lookahead {
            let next = self.index(target + 1);
            if next == target {
                break;
            }
            let (dx, dy) = (
                self.points[next][0] - self.points[target][0],
                self.points[next][1] - self.points[target][1],
            );
            travelled += fm::sqrt(dx * dx + dy * dy);
            target = next;
        }
        let (tx, ty) = (self.points[target][0] - x, self.points[target][1] - y);
        let distance = fm::sqrt(tx * tx + ty * ty).max(0.5);
        let bearing = fm::atan2(ty, tx) - yaw;
        let eta = fm::atan2(fm::sin(bearing), fm::cos(bearing));
        let params = car.params();
        let road_angle = fm::atan2(2.0 * params.body.wheelbase_m * fm::sin(eta), distance);
        let steer = clamp(
            road_angle * params.steering.ratio / params.steering.max_wheel_angle_rad,
            -1.0,
            1.0,
        );

        // Speed: the lowest speed any upcoming corner requires, relaxed by
        // the distance available to brake for it.
        let horizon = speed * speed / (2.0 * cfg.brake_decel) + 20.0;
        let mut allowed = cfg.max_speed;
        let mut i = self.nearest;
        let mut ahead = 0.0;
        let mut steps = 0;
        while ahead < horizon && steps < n {
            let kappa = self.curvature[i].abs();
            if kappa > 1e-6 {
                let corner = fm::sqrt(cfg.lateral_accel / kappa);
                allowed = allowed.min(fm::sqrt(corner * corner + 2.0 * cfg.brake_decel * ahead));
            }
            let next = self.index(i + 1);
            if next == i {
                // End of an open path: plan to stop there.
                allowed = allowed.min(fm::sqrt(2.0 * cfg.brake_decel * ahead));
                break;
            }
            let (dx, dy) = (
                self.points[next][0] - self.points[i][0],
                self.points[next][1] - self.points[i][1],
            );
            ahead += fm::sqrt(dx * dx + dy * dy);
            i = next;
            steps += 1;
        }
        let error = allowed - speed;
        let (throttle, brake) = if self.finished() {
            (0.0, 1.0)
        } else if error >= 0.0 {
            (clamp(error * cfg.throttle_gain, 0.0, 1.0), 0.0)
        } else {
            (0.0, clamp(-error * cfg.brake_gain, 0.0, 1.0))
        };
        InputFrame::from_analog(steer, throttle, brake, 0.0, 0.0)
    }
}

impl PathFollower {
    /// Total length of the followed path, m.
    pub fn length(&self) -> f64 {
        self.length
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_repeatable_and_in_range() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let mut c = Rng::new(8);
        let mut differs = false;
        for _ in 0..1000 {
            let x = a.unit();
            assert_eq!(x, b.unit());
            assert!((0.0..1.0).contains(&x));
            differs |= x != c.unit();
        }
        assert!(differs);
    }

    #[test]
    fn circle_path_has_constant_curvature() {
        let f = PathFollower::circle(
            [10.0, -5.0],
            25.0,
            [35.0, -5.0],
            PathFollowerConfig::default(),
        );
        assert!((f.length() - 2.0 * std::f64::consts::PI * 25.0).abs() < 0.05);
        for k in &f.curvature {
            assert!((k - 1.0 / 25.0).abs() < 1e-4, "curvature {k}");
        }
        assert!((f.points[0][0] - 35.0).abs() < 1e-9);
    }
}
