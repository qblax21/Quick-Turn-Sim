// SPDX-License-Identifier: Apache-2.0
//! The force-feedback torque pipeline.
//!
//! The simulation produces raw physical steering torque at the physics
//! rate. This pipeline turns it into a device command:
//!
//! ```text
//! raw torque (N·m)
//!   → gain
//!   → low-pass filter
//!   → slew-rate limiter
//!   → + damping, friction, inertia   (from the physical wheel's motion)
//!   → + road-detail and kerb channels (optional)
//!   → ÷ device peak torque
//!   → soft clip
//!   → minimum force                  (optional, for weak or belt-driven bases)
//!   → sign
//!   → device
//! ```
//!
//! Every intermediate signal is returned in an [`FfbSample`] so telemetry
//! can record the signal before and after filtering and clipping.
//!
//! Sign convention: positive torque pushes the steering wheel to the left
//! (counter-clockwise seen by the driver), matching the simulation's
//! steering sign. A device that uses the opposite sense is handled by
//! `invert` in the configuration.

use serde::Deserialize;

/// Pipeline settings, mirroring the files in `data/ffb`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FfbConfig {
    /// Identifier, equal to the data file stem.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Free-text description.
    #[serde(default)]
    pub description: String,
    /// False while the preset has not been tuned on the real device.
    pub tuned: bool,
    /// Reverse the output sign for devices whose positive force is to the
    /// right.
    pub invert: bool,
    /// Torque the device produces at full output, N·m.
    pub device_max_torque_nm: f64,
    /// Total rotation the device is set to, lock to lock, rad.
    pub device_rotation_rad: f64,
    /// Multiplier on the physical steering torque.
    pub gain: f64,
    /// Low-pass cutoff on the steering torque, Hz. Zero disables it.
    pub lowpass_cutoff_hz: f64,
    /// Largest rate of change of the steering torque, N·m/s. Zero disables
    /// the limiter.
    pub slew_limit_nm_per_s: f64,
    /// Output level, 0 to 1, where the soft clip starts to compress.
    pub soft_clip_knee: f64,
    /// Damping added at the wheel, N·m per rad/s.
    pub damping_nm_s_per_rad: f64,
    /// Friction added at the wheel, N·m.
    pub friction_nm: f64,
    /// Wheel speed over which the friction effect reaches full strength,
    /// rad/s.
    pub friction_smoothing_rad_per_s: f64,
    /// Inertia added at the wheel, N·m per rad/s².
    pub inertia_nm_s2_per_rad: f64,
    /// Cutoff of the filter on measured wheel velocity, Hz.
    pub velocity_filter_hz: f64,
    /// Road-detail channel gain, N·m per newton of left-right front load
    /// difference above the high-pass corner.
    pub road_detail_gain_nm_per_n: f64,
    /// Kerb channel gain, N·m per newton, applied while a front wheel is on
    /// a kerb surface.
    pub kerb_gain_nm_per_n: f64,
    /// High-pass corner of the detail channels, Hz.
    pub detail_highpass_hz: f64,
    /// Limit on the summed detail channels, N·m.
    pub detail_limit_nm: f64,
    /// How often the device is updated, Hz.
    pub device_update_rate_hz: f64,
    /// Smallest output, as a fraction of device peak, that any non-zero
    /// command is raised to. It lifts small forces above the friction and
    /// dead zone of a weak base. Zero disables it. It changes only what is
    /// sent to the device, never the simulation.
    #[serde(default)]
    pub min_force: f64,
}

impl FfbConfig {
    /// Checks that values are inside the ranges the pipeline assumes.
    pub fn validate(&self) -> Result<(), String> {
        let positive = |name: &str, v: f64| {
            if v.is_finite() && v > 0.0 {
                Ok(())
            } else {
                Err(format!("{name} must be positive and finite, got {v}"))
            }
        };
        let non_negative = |name: &str, v: f64| {
            if v.is_finite() && v >= 0.0 {
                Ok(())
            } else {
                Err(format!("{name} must be zero or positive, got {v}"))
            }
        };
        positive("device_max_torque_nm", self.device_max_torque_nm)?;
        positive("device_rotation_rad", self.device_rotation_rad)?;
        non_negative("gain", self.gain)?;
        non_negative("lowpass_cutoff_hz", self.lowpass_cutoff_hz)?;
        non_negative("slew_limit_nm_per_s", self.slew_limit_nm_per_s)?;
        if !(0.0..1.0).contains(&self.soft_clip_knee) {
            return Err(format!(
                "soft_clip_knee must be in [0, 1), got {}",
                self.soft_clip_knee
            ));
        }
        non_negative("damping_nm_s_per_rad", self.damping_nm_s_per_rad)?;
        non_negative("friction_nm", self.friction_nm)?;
        positive(
            "friction_smoothing_rad_per_s",
            self.friction_smoothing_rad_per_s,
        )?;
        non_negative("inertia_nm_s2_per_rad", self.inertia_nm_s2_per_rad)?;
        positive("velocity_filter_hz", self.velocity_filter_hz)?;
        non_negative("road_detail_gain_nm_per_n", self.road_detail_gain_nm_per_n)?;
        non_negative("kerb_gain_nm_per_n", self.kerb_gain_nm_per_n)?;
        positive("detail_highpass_hz", self.detail_highpass_hz)?;
        non_negative("detail_limit_nm", self.detail_limit_nm)?;
        positive("device_update_rate_hz", self.device_update_rate_hz)?;
        if !(0.0..=MAX_MIN_FORCE).contains(&self.min_force) {
            return Err(format!(
                "min_force must be between 0 and {MAX_MIN_FORCE}, got {}",
                self.min_force
            ));
        }
        Ok(())
    }
}

/// What the pipeline is given each physics tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FfbInput {
    /// Raw steering torque at the column from the simulation, N·m.
    pub steering_torque_nm: f64,
    /// Angle of the physical wheel, rad, positive left. `None` when no
    /// device is connected or it cannot report position.
    pub device_angle_rad: Option<f64>,
    /// Left minus right front tire load, N. Source of the detail channels.
    pub front_load_difference_n: f64,
    /// Share of the front wheels on a kerb surface, 0, 0.5 or 1.
    pub kerb_contact: f64,
}

/// Every stage of the pipeline for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FfbSample {
    /// Raw steering torque from the simulation, N·m.
    pub raw_nm: f64,
    /// After gain, before filtering, N·m.
    pub scaled_nm: f64,
    /// After the low-pass filter and slew limiter, N·m.
    pub filtered_nm: f64,
    /// Damping, friction and inertia effects, N·m.
    pub effects_nm: f64,
    /// Road-detail and kerb channels, N·m.
    pub detail_nm: f64,
    /// Total requested output as a fraction of device peak torque, before
    /// the soft clip. May exceed ±1.
    pub requested: f64,
    /// Output sent to the device, −1 to 1, in the simulation's sign
    /// convention (before `invert`).
    pub output: f64,
    /// How much the soft clip removed: `|requested|` minus the clipped
    /// level before the minimum force is added. Never negative.
    pub clipped: f64,
    /// Filtered velocity of the physical wheel, rad/s.
    pub wheel_velocity_rad_s: f64,
}

impl FfbSample {
    /// The command for the device, with `invert` applied.
    pub fn device_command(&self, config: &FfbConfig) -> f32 {
        let v = if config.invert {
            -self.output
        } else {
            self.output
        };
        v as f32
    }
}

/// Smooth saturation: identity up to `knee`, then a `tanh` curve that
/// approaches ±1 without ever reaching it. Continuous in value and slope.
pub fn soft_clip(x: f64, knee: f64) -> f64 {
    let knee = knee.clamp(0.0, 0.999);
    let magnitude = x.abs();
    if magnitude <= knee {
        return x;
    }
    let span = 1.0 - knee;
    let out = knee + span * ((magnitude - knee) / span).tanh();
    out.copysign(x)
}

/// Largest minimum-force setting accepted. Above this the wheel would
/// have little range left to say anything with.
pub const MAX_MIN_FORCE: f64 = 0.3;

/// Output level, as a fraction of full scale, over which the minimum force
/// fades in. Commands closer to zero are scaled linearly, so the output
/// stays continuous through zero and the wheel does not chatter about the
/// centre.
pub const MIN_FORCE_RAMP: f64 = 0.01;

/// Raises any non-zero output to at least `min_force`, keeping the order
/// of magnitudes and the full-scale end: `|y| = m + (1 − m)·|x|`, faded in
/// linearly below [`MIN_FORCE_RAMP`] so it is continuous at zero.
pub fn apply_min_force(x: f64, min_force: f64) -> f64 {
    let m = min_force.clamp(0.0, MAX_MIN_FORCE);
    if m <= 0.0 {
        return x;
    }
    let a = x.abs().min(1.0);
    let lifted = |v: f64| m + (1.0 - m) * v;
    let y = if a >= MIN_FORCE_RAMP {
        lifted(a)
    } else {
        a / MIN_FORCE_RAMP * lifted(MIN_FORCE_RAMP)
    };
    y.copysign(x)
}

/// One-pole filter coefficient for a cutoff and step: the fraction of the
/// way to the input the state moves each step.
fn lowpass_alpha(cutoff_hz: f64, dt: f64) -> f64 {
    let tau = 1.0 / (2.0 * std::f64::consts::PI * cutoff_hz);
    dt / (tau + dt)
}

/// The pipeline state. Not part of the deterministic simulation state.
#[derive(Clone, Debug)]
pub struct FfbPipeline {
    config: FfbConfig,
    dt: f64,
    lowpass: f64,
    slewed: f64,
    primed: bool,
    prev_angle: Option<f64>,
    velocity: f64,
    prev_velocity: f64,
    accel: f64,
    detail_prev_in: f64,
    detail_hp: f64,
}

impl FfbPipeline {
    /// Creates a pipeline stepped `tick_rate_hz` times per second.
    pub fn new(config: FfbConfig, tick_rate_hz: u32) -> Result<Self, String> {
        config.validate()?;
        Ok(Self {
            config,
            dt: 1.0 / f64::from(tick_rate_hz.max(1)),
            lowpass: 0.0,
            slewed: 0.0,
            primed: false,
            prev_angle: None,
            velocity: 0.0,
            prev_velocity: 0.0,
            accel: 0.0,
            detail_prev_in: 0.0,
            detail_hp: 0.0,
        })
    }

    /// The active configuration.
    pub fn config(&self) -> &FfbConfig {
        &self.config
    }

    /// Replaces the configuration, keeping filter state.
    pub fn set_config(&mut self, config: FfbConfig) -> Result<(), String> {
        config.validate()?;
        self.config = config;
        Ok(())
    }

    /// Clears all filter state, for example after the car is reset.
    pub fn reset(&mut self) {
        self.lowpass = 0.0;
        self.slewed = 0.0;
        self.primed = false;
        self.prev_angle = None;
        self.velocity = 0.0;
        self.prev_velocity = 0.0;
        self.accel = 0.0;
        self.detail_prev_in = 0.0;
        self.detail_hp = 0.0;
    }

    /// Processes one physics tick.
    pub fn step(&mut self, input: &FfbInput) -> FfbSample {
        let c = &self.config;
        let dt = self.dt;
        let raw = if input.steering_torque_nm.is_finite() {
            input.steering_torque_nm
        } else {
            0.0
        };
        let scaled = raw * c.gain;
        let load_in = if input.front_load_difference_n.is_finite() {
            input.front_load_difference_n
        } else if self.primed {
            self.detail_prev_in
        } else {
            0.0
        };

        // Low-pass, then slew limit.
        if !self.primed {
            self.lowpass = scaled;
            self.slewed = scaled;
            self.detail_prev_in = load_in;
            self.primed = true;
        }
        if c.lowpass_cutoff_hz > 0.0 {
            self.lowpass += lowpass_alpha(c.lowpass_cutoff_hz, dt) * (scaled - self.lowpass);
        } else {
            self.lowpass = scaled;
        }
        if c.slew_limit_nm_per_s > 0.0 {
            let step = c.slew_limit_nm_per_s * dt;
            self.slewed += (self.lowpass - self.slewed).clamp(-step, step);
        } else {
            self.slewed = self.lowpass;
        }
        let filtered = self.slewed;

        // Wheel motion from the measured angle.
        let raw_velocity = match (input.device_angle_rad, self.prev_angle) {
            (Some(now), Some(before)) if now.is_finite() => (now - before) / dt,
            _ => 0.0,
        };
        self.prev_angle = input.device_angle_rad.filter(|a| a.is_finite());
        let alpha = lowpass_alpha(c.velocity_filter_hz, dt);
        self.velocity += alpha * (raw_velocity - self.velocity);
        let raw_accel = (self.velocity - self.prev_velocity) / dt;
        self.prev_velocity = self.velocity;
        self.accel += alpha * (raw_accel - self.accel);
        let effects = -c.damping_nm_s_per_rad * self.velocity
            - c.friction_nm * (self.velocity / c.friction_smoothing_rad_per_s).tanh()
            - c.inertia_nm_s2_per_rad * self.accel;

        // Detail channels: high-passed front load difference.
        let hp_alpha = 1.0 / (1.0 + 2.0 * std::f64::consts::PI * c.detail_highpass_hz * dt);
        let load = load_in;
        self.detail_hp = hp_alpha * (self.detail_hp + load - self.detail_prev_in);
        self.detail_prev_in = load;
        let detail_gain =
            c.road_detail_gain_nm_per_n + c.kerb_gain_nm_per_n * input.kerb_contact.clamp(0.0, 1.0);
        let detail = (self.detail_hp * detail_gain).clamp(-c.detail_limit_nm, c.detail_limit_nm);

        let requested = (filtered + effects + detail) / c.device_max_torque_nm;
        let clipped_level = soft_clip(requested, c.soft_clip_knee);
        let output = apply_min_force(clipped_level, c.min_force);
        FfbSample {
            raw_nm: raw,
            scaled_nm: scaled,
            filtered_nm: filtered,
            effects_nm: effects,
            detail_nm: detail,
            requested,
            output,
            clipped: (requested.abs() - clipped_level.abs()).max(0.0),
            wheel_velocity_rad_s: self.velocity,
        }
    }
}

/// A plain configuration for tests: no filtering, no effects.
#[doc(hidden)]
pub fn test_config() -> FfbConfig {
    FfbConfig {
        id: "test".into(),
        name: "Test".into(),
        description: String::new(),
        tuned: false,
        invert: false,
        device_max_torque_nm: 10.0,
        device_rotation_rad: 15.7,
        gain: 1.0,
        lowpass_cutoff_hz: 0.0,
        slew_limit_nm_per_s: 0.0,
        soft_clip_knee: 0.8,
        damping_nm_s_per_rad: 0.0,
        friction_nm: 0.0,
        friction_smoothing_rad_per_s: 0.5,
        inertia_nm_s2_per_rad: 0.0,
        velocity_filter_hz: 40.0,
        road_detail_gain_nm_per_n: 0.0,
        kerb_gain_nm_per_n: 0.0,
        detail_highpass_hz: 8.0,
        detail_limit_nm: 2.0,
        device_update_rate_hz: 500.0,
        min_force: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn torque(nm: f64) -> FfbInput {
        FfbInput {
            steering_torque_nm: nm,
            ..FfbInput::default()
        }
    }

    #[test]
    fn soft_clip_is_identity_below_the_knee_and_never_reaches_one() {
        assert_eq!(soft_clip(0.5, 0.8), 0.5);
        assert_eq!(soft_clip(-0.8, 0.8), -0.8);
        let mut previous = 0.8;
        for i in 1..2000 {
            let x = 0.8 + i as f64 * 0.01;
            let y = soft_clip(x, 0.8);
            assert!(y > previous || y >= 1.0 - 1e-12, "must keep rising");
            assert!(y <= 1.0);
            previous = y;
        }
        assert!(
            soft_clip(1.5, 0.8) < 1.0,
            "moderate overdrive stays below full scale"
        );
        assert_eq!(soft_clip(-3.0, 0.8), -soft_clip(3.0, 0.8));
    }

    #[test]
    fn soft_clip_has_no_kink_at_the_knee() {
        let knee = 0.7;
        let h = 1e-6;
        let below = (soft_clip(knee, knee) - soft_clip(knee - h, knee)) / h;
        let above = (soft_clip(knee + h, knee) - soft_clip(knee, knee)) / h;
        assert!((below - 1.0).abs() < 1e-6);
        assert!(
            (above - 1.0).abs() < 1e-4,
            "slope just above the knee: {above}"
        );
    }

    #[test]
    fn gain_and_device_torque_set_the_scale() {
        let mut cfg = test_config();
        cfg.gain = 0.5;
        let mut p = FfbPipeline::new(cfg, 1000).unwrap();
        let s = p.step(&torque(8.0));
        assert_eq!(s.raw_nm, 8.0);
        assert_eq!(s.scaled_nm, 4.0);
        assert!((s.requested - 0.4).abs() < 1e-12);
        assert!((s.output - 0.4).abs() < 1e-12);
        assert_eq!(s.clipped, 0.0);
    }

    #[test]
    fn overdrive_is_compressed_and_reported() {
        let mut p = FfbPipeline::new(test_config(), 1000).unwrap();
        let s = p.step(&torque(25.0));
        assert!((s.requested - 2.5).abs() < 1e-12);
        assert!(s.output > 0.8 && s.output < 1.0);
        assert!((s.clipped - (2.5 - s.output)).abs() < 1e-12);
    }

    #[test]
    fn invert_flips_only_the_device_command() {
        let mut cfg = test_config();
        cfg.invert = true;
        let mut p = FfbPipeline::new(cfg.clone(), 1000).unwrap();
        let s = p.step(&torque(3.0));
        assert!(s.output > 0.0);
        assert!(s.device_command(&cfg) < 0.0);
    }

    #[test]
    fn low_pass_reaches_63_percent_after_one_time_constant() {
        let mut cfg = test_config();
        cfg.lowpass_cutoff_hz = 10.0;
        let mut p = FfbPipeline::new(cfg, 1000).unwrap();
        p.step(&torque(0.0));
        let tau = 1.0 / (2.0 * std::f64::consts::PI * 10.0);
        let ticks = (tau * 1000.0).round() as usize;
        let mut s = FfbSample::default();
        for _ in 0..ticks {
            s = p.step(&torque(5.0));
        }
        assert!(
            (s.filtered_nm / 5.0 - 0.632).abs() < 0.03,
            "{}",
            s.filtered_nm
        );
        for _ in 0..1000 {
            s = p.step(&torque(5.0));
        }
        assert!((s.filtered_nm - 5.0).abs() < 1e-6);
    }

    #[test]
    fn slew_limiter_bounds_the_rate_of_change() {
        let mut cfg = test_config();
        cfg.slew_limit_nm_per_s = 100.0;
        let mut p = FfbPipeline::new(cfg, 1000).unwrap();
        p.step(&torque(0.0));
        let mut previous = 0.0;
        for i in 0..60 {
            let s = p.step(&torque(5.0));
            assert!(s.filtered_nm - previous <= 0.1 + 1e-12);
            previous = s.filtered_nm;
            if i >= 50 {
                assert!((s.filtered_nm - 5.0).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn first_sample_does_not_ramp_from_zero() {
        let mut cfg = test_config();
        cfg.lowpass_cutoff_hz = 5.0;
        cfg.slew_limit_nm_per_s = 10.0;
        let mut p = FfbPipeline::new(cfg, 1000).unwrap();
        assert_eq!(p.step(&torque(4.0)).filtered_nm, 4.0);
    }

    #[test]
    fn damping_and_friction_oppose_wheel_motion() {
        let mut cfg = test_config();
        cfg.damping_nm_s_per_rad = 0.5;
        cfg.friction_nm = 0.3;
        let mut p = FfbPipeline::new(cfg, 1000).unwrap();
        let mut s = FfbSample::default();
        // Wheel turning left at 2 rad/s.
        for i in 0..500 {
            s = p.step(&FfbInput {
                device_angle_rad: Some(2.0 * i as f64 * 0.001),
                ..FfbInput::default()
            });
        }
        assert!((s.wheel_velocity_rad_s - 2.0).abs() < 0.01);
        // 0.5 × 2 of damping plus nearly all of the 0.3 friction.
        assert!((s.effects_nm + 1.3).abs() < 0.01, "{}", s.effects_nm);
        // Losing the device stops the effects rather than spiking them.
        let lost = p.step(&FfbInput::default());
        assert!(lost.effects_nm.abs() <= s.effects_nm.abs() + 1e-9);
    }

    #[test]
    fn inertia_opposes_wheel_acceleration() {
        let mut cfg = test_config();
        cfg.inertia_nm_s2_per_rad = 0.02;
        let mut p = FfbPipeline::new(cfg, 1000).unwrap();
        let mut s = FfbSample::default();
        // Constant angular acceleration of 10 rad/s² to the left.
        for i in 0..800 {
            let t = i as f64 * 0.001;
            s = p.step(&FfbInput {
                device_angle_rad: Some(0.5 * 10.0 * t * t),
                ..FfbInput::default()
            });
        }
        assert!((s.effects_nm + 0.2).abs() < 0.02, "{}", s.effects_nm);
    }

    #[test]
    fn detail_channel_passes_vibration_and_blocks_steady_load_difference() {
        let mut cfg = test_config();
        cfg.road_detail_gain_nm_per_n = 0.001;
        cfg.kerb_gain_nm_per_n = 0.004;
        let mut p = FfbPipeline::new(cfg, 1000).unwrap();
        // A steady 2000 N difference (cornering) decays to nothing.
        let mut s = FfbSample::default();
        for _ in 0..2000 {
            s = p.step(&FfbInput {
                front_load_difference_n: 2000.0,
                ..FfbInput::default()
            });
        }
        assert!(s.detail_nm.abs() < 1e-3, "{}", s.detail_nm);
        // A 30 Hz, ±300 N vibration comes through; more on a kerb.
        let mut peak_road: f64 = 0.0;
        let mut peak_kerb: f64 = 0.0;
        for i in 0..2000 {
            let t = i as f64 * 0.001;
            let load = 2000.0 + 300.0 * (2.0 * std::f64::consts::PI * 30.0 * t).sin();
            let on_kerb = i >= 1000;
            let s = p.step(&FfbInput {
                front_load_difference_n: load,
                kerb_contact: if on_kerb { 1.0 } else { 0.0 },
                ..FfbInput::default()
            });
            if i > 200 && !on_kerb {
                peak_road = peak_road.max(s.detail_nm.abs());
            } else if i > 1200 {
                peak_kerb = peak_kerb.max(s.detail_nm.abs());
            }
        }
        assert!(
            peak_road > 0.2 && peak_road < 0.31,
            "road detail {peak_road}"
        );
        assert!(
            peak_kerb > 3.0 * peak_road,
            "kerb {peak_kerb} vs road {peak_road}"
        );
        assert!(peak_kerb <= 2.0 + 1e-12, "detail limit");
    }

    #[test]
    fn min_force_lifts_small_commands_and_keeps_full_scale() {
        assert_eq!(apply_min_force(0.3, 0.0), 0.3);
        assert_eq!(apply_min_force(0.0, 0.1), 0.0);
        // Above the ramp: m + (1 - m)·x.
        assert!((apply_min_force(0.2, 0.1) - 0.28).abs() < 1e-12);
        assert!((apply_min_force(-0.2, 0.1) + 0.28).abs() < 1e-12);
        assert!((apply_min_force(1.0, 0.1) - 1.0).abs() < 1e-12);
        // Continuous at the end of the ramp and monotonic through zero.
        let h = 1e-9;
        let below = apply_min_force(MIN_FORCE_RAMP - h, 0.1);
        let above = apply_min_force(MIN_FORCE_RAMP + h, 0.1);
        assert!((above - below).abs() < 1e-6);
        let mut previous = -1.0;
        for i in -1000..=1000 {
            let y = apply_min_force(f64::from(i) / 1000.0, 0.15);
            assert!(y >= previous);
            previous = y;
        }
    }

    #[test]
    fn min_force_is_not_counted_as_clipping() {
        let mut cfg = test_config();
        cfg.min_force = 0.2;
        let mut p = FfbPipeline::new(cfg, 1000).unwrap();
        let s = p.step(&torque(1.0));
        assert!((s.requested - 0.1).abs() < 1e-12);
        assert!((s.output - 0.28).abs() < 1e-12);
        assert_eq!(s.clipped, 0.0);
        let s = p.step(&torque(25.0));
        assert!(s.clipped > 1.0);
        let mut cfg = test_config();
        cfg.min_force = 0.5;
        assert!(FfbPipeline::new(cfg, 1000).is_err());
    }

    #[test]
    fn bad_inputs_and_bad_configs_are_contained() {
        let mut p = FfbPipeline::new(test_config(), 1000).unwrap();
        let s = p.step(&FfbInput {
            steering_torque_nm: f64::NAN,
            device_angle_rad: Some(f64::INFINITY),
            front_load_difference_n: f64::NAN,
            kerb_contact: 7.0,
        });
        assert!(s.output.is_finite() && s.output == 0.0);
        let mut cfg = test_config();
        cfg.soft_clip_knee = 1.0;
        assert!(FfbPipeline::new(cfg, 1000).is_err());
        let mut cfg = test_config();
        cfg.device_max_torque_nm = 0.0;
        assert!(FfbPipeline::new(cfg, 1000).is_err());
    }
}
