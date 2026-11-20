// SPDX-License-Identifier: Apache-2.0
//! The scripted manoeuvres behind each validation metric.
//!
//! Each manoeuvre drives the headless session with a simple, documented
//! driver model and extracts one number. The driver models are deliberately
//! plain: a result that depends on clever driving would say more about the
//! driver than the car. Each measurement carries a short description of how
//! it was driven.

use crate::{Measurement, Metric, TargetSet};
use qts_core::constants::GRAVITY_M_S2;
use qts_core::input::InputFrame;
use qts_core::step::Simulation;
use qts_sim::drivers::{Driver, PathFollower, PathFollowerConfig};
use qts_sim::session::{Session, SessionConfig};
use qts_sim::track::MarkerDef;
use qts_sim::DataRoot;
/// 60 mph in m/s.
const SIXTY_MPH: f64 = 26.8224;
/// 70 mph in m/s.
const SEVENTY_MPH: f64 = 31.2928;
use qts_vehicle::params::DrivetrainParams;
use qts_vehicle::Assists;

/// Results that several metrics share, so a manoeuvre runs once.
#[derive(Clone, Debug, Default)]
pub struct Cache {
    circle: Option<CircleRun>,
}

#[derive(Clone, Debug)]
struct CircleRun {
    radius: f64,
    max_lateral_g: f64,
    speed_at_max: f64,
    gradient_deg_per_g: Option<f64>,
    gradient_points: usize,
}

const BOTH_ASSISTS: Assists = Assists {
    auto_clutch: true,
    auto_shift: true,
    traction_control: true,
    engine_damage: false,
};

fn new_session(root: &DataRoot, set: &TargetSet, assists: Assists) -> Result<Session, String> {
    let config = SessionConfig {
        assists,
        tire_start_temp_c: Some(set.tire_temp_c),
        ..SessionConfig::default()
    };
    Session::from_data(root, &set.car, &set.track, config)
}

/// Small steering correction that keeps the car on the line through
/// `origin` along `heading`.
fn hold_straight(session: &Session, origin: [f64; 2], heading: f64) -> f64 {
    let s = &session.car().state;
    let (sin, cos) = heading.sin_cos();
    let lateral = -(s.pos.x - origin[0]) * sin + (s.pos.y - origin[1]) * cos;
    let mut yaw_error = s.rot.yaw() - heading;
    while yaw_error > std::f64::consts::PI {
        yaw_error -= 2.0 * std::f64::consts::PI;
    }
    while yaw_error < -std::f64::consts::PI {
        yaw_error += 2.0 * std::f64::consts::PI;
    }
    (-(0.02 * lateral + 0.6 * yaw_error)).clamp(-0.1, 0.1)
}

/// Distance travelled from `origin` along `heading`.
fn distance_along(session: &Session, origin: [f64; 2], heading: f64) -> f64 {
    let s = &session.car().state;
    let (sin, cos) = heading.sin_cos();
    (s.pos.x - origin[0]) * cos + (s.pos.y - origin[1]) * sin
}

/// Largest positive slip ratio among the wheels.
fn worst_drive_slip(session: &Session) -> f64 {
    session
        .car()
        .output()
        .wheels
        .iter()
        .map(|w| w.slip_ratio)
        .fold(0.0, f64::max)
}

/// A throttle limiter that backs off quickly when the driven wheels spin
/// and comes back more slowly: the simplest traction-limited launch a
/// driver could manage.
struct SlipLimiter {
    limit: f64,
    target_slip: f64,
}

impl SlipLimiter {
    fn new() -> Self {
        Self {
            limit: 1.0,
            target_slip: 0.11,
        }
    }

    fn throttle(&mut self, session: &Session) -> f64 {
        // Proportional-integral control of slip toward the target.
        let dt = session.tick_dt();
        let error = self.target_slip - worst_drive_slip(session);
        self.limit = (self.limit + 25.0 * error * dt).clamp(0.15, 1.0);
        (self.limit + 3.0 * error).clamp(0.0, 1.0)
    }
}

fn circle_run(root: &DataRoot, set: &TargetSet) -> Result<CircleRun, String> {
    let mut session = new_session(root, set, BOTH_ASSISTS)?;
    let (center, radius) = session
        .track()
        .def()
        .markers
        .iter()
        .find_map(|m| match m {
            MarkerDef::Circle {
                center_x,
                center_y,
                radius_m,
                ..
            } => Some(([*center_x, *center_y], *radius_m)),
            _ => None,
        })
        .ok_or_else(|| format!("track `{}` has no circle marker for a skidpad", set.track))?;
    // Start at the bottom of the circle heading along it, counter-clockwise.
    let start = [center[0], center[1] - radius];
    session.car_mut().place(start[0], start[1], 0.0, 0.0);
    session.car_mut().set_tire_temperature(set.tire_temp_c);
    session.car_mut().set_forward_speed(6.0);
    let config = PathFollowerConfig {
        max_speed: 6.0,
        // The follower's own corner-speed planning is switched off by a
        // very high planned lateral acceleration; the speed ramp below is
        // what finds the limit.
        lateral_accel: 60.0,
        brake_decel: 6.0,
        lookahead_base: 2.5,
        lookahead_gain: 0.25,
        throttle_gain: 0.6,
        brake_gain: 0.3,
        closed: true,
    };
    let mut driver = PathFollower::circle(center, radius, start, config);
    let rate = session.tick_rate_hz();
    let window = rate as usize;
    let ramp = 0.12; // m/s of target speed per second
    let mut samples: Vec<(f64, f64)> = Vec::new(); // (lateral g, road-wheel steer in degrees)
    let mut best = (0.0f64, 0.0f64);
    let (mut sum_ay, mut sum_steer, mut sum_speed, mut sum_error, mut count) =
        (0.0, 0.0, 0.0, 0.0, 0usize);
    let max_ticks = 300 * u64::from(rate);
    for tick in 0..max_ticks {
        let target_speed = 6.0 + ramp * tick as f64 / f64::from(rate);
        driver.set_max_speed(target_speed);
        let frame = driver.frame(&session);
        session.step(&frame);
        if !session.is_finite() {
            return Err("the simulation became unstable on the skidpad".into());
        }
        let car = session.car();
        let out = car.output();
        let (dx, dy) = (car.state.pos.x - center[0], car.state.pos.y - center[1]);
        let radial_error = ((dx * dx + dy * dy).sqrt() - radius).abs();
        sum_ay += out.accel_body[1];
        // Reference steer angle: steering-wheel angle over the steering
        // ratio, so compliance in the steering system counts.
        sum_steer += out.steering_wheel_angle / car.params().steering.ratio;
        sum_speed += out.speed;
        sum_error += radial_error;
        count += 1;
        if count == window {
            let n = count as f64;
            let (ay, steer, speed, error) =
                (sum_ay / n, sum_steer / n, sum_speed / n, sum_error / n);
            (sum_ay, sum_steer, sum_speed, sum_error, count) = (0.0, 0.0, 0.0, 0.0, 0);
            if error < 1.5 {
                let g = ay / GRAVITY_M_S2;
                samples.push((g, steer.to_degrees()));
                if g > best.0 {
                    best = (g, speed);
                }
            }
            // Past the limit: the car runs wide or cannot reach the speed.
            let settled = tick > 15 * u64::from(rate);
            if settled && (error > 4.0 || target_speed - speed > 4.0) {
                break;
            }
        }
    }
    if best.0 <= 0.0 {
        return Err("the car never settled on the skidpad circle".into());
    }
    // Least-squares slope of steer angle against lateral acceleration in
    // the linear range.
    let linear: Vec<(f64, f64)> = samples
        .iter()
        .copied()
        .filter(|(g, _)| (0.15..=0.45).contains(g))
        .collect();
    let gradient = if linear.len() >= 4 {
        let n = linear.len() as f64;
        let mean_x = linear.iter().map(|p| p.0).sum::<f64>() / n;
        let mean_y = linear.iter().map(|p| p.1).sum::<f64>() / n;
        let sxx: f64 = linear.iter().map(|p| (p.0 - mean_x).powi(2)).sum();
        let sxy: f64 = linear.iter().map(|p| (p.0 - mean_x) * (p.1 - mean_y)).sum();
        (sxx > 1e-9).then(|| sxy / sxx)
    } else {
        None
    };
    Ok(CircleRun {
        radius,
        max_lateral_g: best.0,
        speed_at_max: best.1,
        gradient_deg_per_g: gradient,
        gradient_points: linear.len(),
    })
}

fn braking_distance(
    root: &DataRoot,
    set: &TargetSet,
    from_speed: f64,
    metric: Metric,
) -> Result<Measurement, String> {
    let mut session = new_session(root, set, BOTH_ASSISTS)?;
    let (x, y, heading) = session.track().spawn();
    let origin = [x, y];
    session.car_mut().set_forward_speed(from_speed + 0.8);
    let rate = session.tick_rate_hz();
    let mut braking_from: Option<f64> = None;
    let mut previous_speed = session.car().output().speed.max(from_speed + 0.8);
    let mut previous_distance = 0.0;
    for _ in 0..(60 * u64::from(rate)) {
        let steer = hold_straight(&session, origin, heading);
        let brake = if braking_from.is_some() { 1.0 } else { 0.0 };
        session.step(&InputFrame::from_analog(steer, 0.0, brake, 0.0, 0.0));
        let speed = session.car().output().vel_body[0];
        let distance = distance_along(&session, origin, heading);
        match braking_from {
            None => {
                if speed <= from_speed {
                    // Interpolate where the speed was crossed during this tick.
                    let span = previous_speed - speed;
                    let frac = if span > 0.0 {
                        (previous_speed - from_speed) / span
                    } else {
                        1.0
                    };
                    braking_from = Some(previous_distance + frac * (distance - previous_distance));
                }
            }
            Some(from) => {
                if speed < 0.05 {
                    return Ok(Measurement {
                        metric,
                        value: distance - from,
                        method: format!("coast down to {:.0} mph in a straight line, then full brake pedal in one step; anti-lock system as fitted; distance from the pedal application to rest", from_speed / 0.44704),
                    });
                }
            }
        }
        previous_speed = speed;
        previous_distance = distance;
    }
    Err("the car did not stop within 60 s".into())
}

/// One foot, m: the rollout US road tests subtract from a standing start.
const ONE_FOOT_M: f64 = 0.3048;

/// One standing start. Returns the time to 60 mph and the time to 60 mph
/// counted from when the car had moved one foot, or `None` if 60 mph was
/// not reached in 30 s.
fn standing_start(
    root: &DataRoot,
    set: &TargetSet,
    launch_rpm: Option<f64>,
    release_s: f64,
) -> Result<Option<(f64, f64)>, String> {
    let manual_clutch = launch_rpm.is_some();
    let assists = Assists {
        auto_clutch: !manual_clutch,
        auto_shift: true,
        traction_control: true,
        engine_damage: false,
    };
    let mut session = new_session(root, set, assists)?;
    let (x, y, heading) = session.track().spawn();
    let origin = [x, y];
    let rate = f64::from(session.tick_rate_hz());
    let mut limiter = SlipLimiter::new();
    // Staging: clutch in, first gear, hold the launch speed.
    let staging_ticks = (1.2 * rate) as u64;
    if let Some(rpm_target) = launch_rpm {
        for _ in 0..staging_ticks {
            let rpm = session.car().output().drive.engine_rpm;
            let throttle = (0.12 + (rpm_target - rpm) * 0.0015).clamp(0.0, 1.0);
            session.step(&InputFrame::from_analog(0.0, throttle, 0.0, 1.0, 0.0).with_gear(1));
        }
    }
    let launch_tick = session.tick();
    let mut previous_speed = 0.0;
    let mut previous_distance = 0.0;
    let mut rollout_at: Option<f64> = None;
    for _ in 0..(30.0 * rate) as u64 {
        let elapsed = (session.tick() - launch_tick) as f64 / rate;
        let clutch = if manual_clutch {
            (1.0 - elapsed / release_s).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // A car with traction control is simply floored; its own system
        // manages the slip, as it would for a driver.
        let throttle = if session.car().params().traction_control.enabled {
            1.0
        } else {
            limiter.throttle(&session)
        };
        let steer = hold_straight(&session, origin, heading);
        session.step(&InputFrame::from_analog(steer, throttle, 0.0, clutch, 0.0));
        let speed = session.car().output().vel_body[0];
        let distance = distance_along(&session, origin, heading);
        let ticks_now = (session.tick() - launch_tick) as f64;
        if rollout_at.is_none() && distance >= ONE_FOOT_M {
            let span = distance - previous_distance;
            let frac = if span > 0.0 {
                (ONE_FOOT_M - previous_distance) / span
            } else {
                1.0
            };
            rollout_at = Some((ticks_now - 1.0 + frac) / rate);
        }
        if speed >= SIXTY_MPH {
            let span = speed - previous_speed;
            let frac = if span > 0.0 {
                (SIXTY_MPH - previous_speed) / span
            } else {
                1.0
            };
            let time = (ticks_now - 1.0 + frac) / rate;
            return Ok(Some((time, time - rollout_at.unwrap_or(0.0))));
        }
        previous_speed = speed;
        previous_distance = distance;
    }
    Ok(None)
}

fn accel_0_60(root: &DataRoot, set: &TargetSet, metric: Metric) -> Result<Measurement, String> {
    let rollout = metric == Metric::Accel0To60Rollout;
    let pick = |t: (f64, f64)| if rollout { t.1 } else { t.0 };
    let car = root.load_car(&set.car).map_err(|e| e.to_string())?.value;
    match car.drivetrain {
        DrivetrainParams::EngineManual(_) => {
            // A driver would try a few launches; so does the harness.
            let mut best: Option<(f64, f64, f64)> = None;
            for rpm in [3500.0, 4500.0, 5500.0] {
                for release in [0.3, 0.6, 1.0] {
                    if let Some(t) = standing_start(root, set, Some(rpm), release)?.map(pick) {
                        if best.is_none_or(|b| t < b.0) {
                            best = Some((t, rpm, release));
                        }
                    }
                }
            }
            let (time, rpm, release) = best.ok_or("60 mph was not reached within 30 s")?;
            let slip_note = if car.traction_control.enabled {
                "accelerator floored, slip managed by the car's traction control"
            } else {
                "throttle backed off when driven-wheel slip exceeds 0.11"
            };
            Ok(Measurement {
                metric,
                value: time,
                method: format!(
                    "best of nine launches: clutch released over {release:.1} s from {rpm:.0} rpm, {slip_note}, automatic upshifts near the limiter; {}",
                    if rollout {
                        "timed from when the car has moved one foot (rollout subtracted)"
                    } else {
                        "timed from the start of clutch release, no rollout"
                    }
                ),
            })
        }
        DrivetrainParams::DualMotorElectric(_) => {
            let time = standing_start(root, set, None, 0.0)?
                .map(pick)
                .ok_or("60 mph was not reached within 30 s")?;
            let timing = if rollout {
                "timed from when the car has moved one foot (rollout subtracted)"
            } else {
                "timed from the pedal application, no rollout"
            };
            Ok(Measurement {
                metric,
                value: time,
                method: if car.traction_control.enabled {
                    format!("accelerator floored from rest, slip managed by the car's traction control; {timing}")
                } else {
                    format!("full accelerator from rest, backed off when wheel slip exceeds 0.11; {timing}")
                },
            })
        }
    }
}

fn top_speed(root: &DataRoot, set: &TargetSet) -> Result<Measurement, String> {
    let mut session = new_session(root, set, BOTH_ASSISTS)?;
    let (x, y, heading) = session.track().spawn();
    let origin = [x, y];
    let rate = session.tick_rate_hz();
    let mut history: Vec<f64> = Vec::new();
    let mut top: f64 = 0.0;
    for tick in 0..(400 * u64::from(rate)) {
        let steer = hold_straight(&session, origin, heading);
        session.step(&InputFrame::from_analog(steer, 1.0, 0.0, 0.0, 0.0));
        if !session.is_finite() {
            return Err("the simulation became unstable at speed".into());
        }
        let speed = session.car().output().vel_body[0];
        top = top.max(speed);
        if (tick + 1) % u64::from(rate) == 0 {
            history.push(speed);
            let n = history.len();
            // Stop once ten seconds add less than 0.05 m/s.
            if n > 20 && history[n - 1] - history[n - 11] < 0.05 {
                return Ok(Measurement {
                    metric: Metric::TopSpeed,
                    value: top,
                    method: format!(
                        "full throttle from rest on level tarmac with automatic clutch and gears until speed stopped rising ({n} s); still air"
                    ),
                });
            }
        }
    }
    Err("speed was still rising after 400 s".into())
}

/// Measures one metric for the car and conditions in `set`.
pub fn measure(
    root: &DataRoot,
    set: &TargetSet,
    metric: Metric,
    cache: &mut Cache,
) -> Result<Measurement, String> {
    match metric {
        Metric::SkidpadLateralG | Metric::UndersteerGradient => {
            if cache.circle.is_none() {
                cache.circle = Some(circle_run(root, set)?);
            }
            let run = cache.circle.as_ref().ok_or("skidpad run missing")?;
            if metric == Metric::SkidpadLateralG {
                Ok(Measurement {
                    metric,
                    value: run.max_lateral_g,
                    method: format!(
                        "{:.0} m radius circle, counter-clockwise, target speed raised 0.12 m/s each second; highest one-second mean lateral acceleration while within 1.5 m of the line (reached at {:.1} m/s)",
                        run.radius, run.speed_at_max
                    ),
                })
            } else {
                let value = run
                    .gradient_deg_per_g
                    .ok_or("too few steady samples between 0.15 g and 0.45 g")?;
                Ok(Measurement {
                    metric,
                    value,
                    method: format!(
                        "same {:.0} m circle run: least-squares slope of steering-wheel angle divided by the steering ratio against lateral acceleration over {} one-second samples between 0.15 g and 0.45 g",
                        run.radius, run.gradient_points
                    ),
                })
            }
        }
        Metric::BrakingDistance60To0 => braking_distance(root, set, SIXTY_MPH, metric),
        Metric::BrakingDistance70To0 => braking_distance(root, set, SEVENTY_MPH, metric),
        Metric::Accel0To60 | Metric::Accel0To60Rollout => accel_0_60(root, set, metric),
        Metric::TopSpeed => top_speed(root, set),
    }
}
