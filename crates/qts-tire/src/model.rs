// SPDX-License-Identifier: Apache-2.0
//! The complete tire: transient layer on top of the steady-state baseline,
//! with thermal and surface effects.
//!
//! The transient layer is the single-contact-point model: the carcass
//! deflects longitudinally (`u`) and laterally (`v`) and the slip seen by
//! the Magic Formula is `u/σκ` and `v/σα`, where the relaxation lengths
//! `σ = K / C_carcass` follow from stiffness. Force therefore builds over
//! rolled distance, and at standstill the tire acts as a spring.

use crate::magic::{characteristics, plough_force, steady_state, Characteristics, Operating};
use crate::params::{SurfaceParams, TireParams};
use crate::thermal::{ThermalInput, ThermalState};
use qts_core::math::{clamp, fm};
use qts_core::snapshot::{Snapshot, SnapshotError, StateReader, StateWriter};

/// Dynamic state of one tire.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TireState {
    /// Longitudinal carcass deflection, m.
    pub deflection_long_m: f64,
    /// Lateral carcass deflection, m.
    pub deflection_lat_m: f64,
    /// Temperatures.
    pub thermal: ThermalState,
    /// Share of the usable tread worn away, 0 (new) to 1.
    pub wear: f64,
}

impl TireState {
    /// A relaxed tire with every thermal node at `temp_c`.
    pub fn new(temp_c: f64) -> Self {
        Self {
            deflection_long_m: 0.0,
            deflection_lat_m: 0.0,
            thermal: ThermalState::uniform(temp_c),
            wear: 0.0,
        }
    }

    /// True when every state value is finite.
    pub fn is_finite(&self) -> bool {
        self.deflection_long_m.is_finite()
            && self.deflection_lat_m.is_finite()
            && self.thermal.is_finite()
            && self.wear.is_finite()
    }
}

impl Snapshot for TireState {
    fn save(&self, w: &mut StateWriter) {
        w.write_f64(self.deflection_long_m);
        w.write_f64(self.deflection_lat_m);
        self.thermal.save(w);
        w.write_f64(self.wear);
    }
    fn load(&mut self, r: &mut StateReader<'_>) -> Result<(), SnapshotError> {
        self.deflection_long_m = r.read_f64()?;
        self.deflection_lat_m = r.read_f64()?;
        self.thermal.load(r)?;
        self.wear = r.read_f64()?;
        if !(0.0..=1.0).contains(&self.wear) {
            return Err(SnapshotError::Invalid("tire wear out of range"));
        }
        Ok(())
    }
}

/// Kinematic inputs to one tire for one tick, in the wheel frame (x along
/// the wheel heading in the ground plane, y to the left, z up).
#[derive(Clone, Copy, Debug)]
pub struct TireInput<'a> {
    /// Contact patch velocity along the wheel heading, m/s.
    pub vx: f64,
    /// Contact patch velocity to the left of the heading, m/s.
    pub vy: f64,
    /// Wheel spin rate, rad/s, positive rolling forward.
    pub omega: f64,
    /// Vertical load, N.
    pub fz: f64,
    /// Rolling radius used for slip and for the wheel torque, m.
    pub rolling_radius_m: f64,
    /// Camber toward the tire's +y side, rad.
    pub camber: f64,
    /// Surface under the contact patch.
    pub surface: &'a SurfaceParams,
    /// Air temperature, °C.
    pub air_temp_c: f64,
    /// Road temperature, °C.
    pub road_temp_c: f64,
}

/// Forces and diagnostics from one tire for one tick.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TireOutput {
    /// Longitudinal force on the tire from the road, N.
    pub fx: f64,
    /// Lateral force on the tire from the road, N.
    pub fy: f64,
    /// Aligning moment about the vertical axis, N·m.
    pub mz: f64,
    /// Magnitude of the rolling-resistance torque about the spin axis, N·m.
    /// It always opposes rotation; the wheel-spin solver applies the sign.
    pub rolling_resistance_torque: f64,
    /// Transient slip ratio fed to the force model.
    pub slip_ratio: f64,
    /// Transient slip angle fed to the force model, rad.
    pub slip_angle: f64,
    /// Kinematic slip ratio (what a steady-state model would see).
    pub slip_ratio_kinematic: f64,
    /// Kinematic slip angle, rad.
    pub slip_angle_kinematic: f64,
    /// Longitudinal relaxation length in use, m.
    pub relaxation_long_m: f64,
    /// Lateral relaxation length in use, m.
    pub relaxation_lat_m: f64,
    /// Stiffness and peak friction at this load.
    pub characteristics: Characteristics,
    /// Power dissipated by sliding in the contact patch, W.
    pub sliding_power_w: f64,
    /// Pneumatic trail, m.
    pub pneumatic_trail: f64,
}

/// Speed floor used when turning slip velocity into a kinematic slip for
/// diagnostics only.
const DIAGNOSTIC_SPEED_FLOOR: f64 = 0.5;

/// Vertical stiffness at the tire's current pressure, N/m.
pub fn vertical_stiffness(p: &TireParams, state: &TireState) -> f64 {
    let dpi = state.thermal.dpi(p);
    (p.vertical.stiffness_n_per_m * (1.0 + p.pressure.pfz1 * dpi)).max(0.0)
}

/// Vertical load from radial deflection and its rate. Never negative: the
/// tire cannot pull on the road.
pub fn vertical_load(
    p: &TireParams,
    state: &TireState,
    deflection_m: f64,
    rate_m_per_s: f64,
) -> f64 {
    if deflection_m <= 0.0 {
        return 0.0;
    }
    (vertical_stiffness(p, state) * deflection_m + p.vertical.damping_n_s_per_m * rate_m_per_s)
        .max(0.0)
}

/// Advances one tire by one tick and returns its forces.
pub fn step(p: &TireParams, state: &mut TireState, input: &TireInput<'_>, dt: f64) -> TireOutput {
    let surface = input.surface;
    let fz = input.fz.max(0.0);
    let dpi = state.thermal.dpi(p);
    let wear = state.wear;
    let mut op = Operating {
        fz,
        kappa: 0.0,
        tan_alpha: 0.0,
        camber: input.camber,
        dpi,
        grip_factor: state.thermal.grip_factor(&p.thermal)
            * (1.0 - p.wear.grip_loss_at_full_wear * wear),
        stiffness_factor: state.thermal.stiffness_factor(&p.thermal)
            * (1.0 + p.wear.stiffness_gain_at_full_wear * wear),
    };
    let tread_scale = 1.0 - p.wear.tread_heat_capacity_share * wear;

    let vsx = input.vx - input.omega * input.rolling_radius_m;
    let vsy = input.vy;
    let speed = input.vx.abs();
    let floor = speed.max(DIAGNOSTIC_SPEED_FLOOR);
    let kappa_kin = -vsx / floor;
    let alpha_kin = fm::atan2(-vsy, floor);

    let mut out = TireOutput {
        slip_ratio_kinematic: kappa_kin,
        slip_angle_kinematic: alpha_kin,
        ..TireOutput::default()
    };

    if fz <= 0.0 {
        // Airborne: the carcass relaxes and only the thermal model runs.
        state.deflection_long_m = 0.0;
        state.deflection_lat_m = 0.0;
        out.characteristics = characteristics(p, surface, &op);
        state.thermal.step(
            &p.thermal,
            &ThermalInput {
                sliding_power_w: 0.0,
                rolling_power_w: 0.0,
                speed_m_per_s: (input.omega * input.rolling_radius_m).abs().max(speed),
                in_contact: false,
                air_temp_c: input.air_temp_c,
                road_temp_c: input.road_temp_c,
                tread_scale,
            },
            dt,
        );
        return out;
    }

    let ch = characteristics(p, surface, &op);
    let tr = &p.transient;
    let carcass_scale = (1.0 + p.pressure.carcass_stiffness_sensitivity * dpi).max(0.1);
    let sigma_k = (ch.kx / (tr.carcass_stiffness_long_n_per_m * carcass_scale))
        .max(tr.min_relaxation_length_m);
    let sigma_a = (ch.ky / (tr.carcass_stiffness_lat_n_per_m * carcass_scale))
        .max(tr.min_relaxation_length_m);

    // Implicit update of the deflections: stable at any speed, and a pure
    // integrator at standstill.
    let u_limit = sigma_k * tr.slip_ratio_limit;
    let v_limit = sigma_a * tr.slip_tangent_limit;
    let u = (state.deflection_long_m - vsx * dt) / (1.0 + speed * dt / sigma_k);
    let v = (state.deflection_lat_m - vsy * dt) / (1.0 + speed * dt / sigma_a);
    state.deflection_long_m = clamp(u, -u_limit, u_limit);
    state.deflection_lat_m = clamp(v, -v_limit, v_limit);

    // Low-speed damping, fading out above the threshold speed.
    let k_low = if speed < tr.low_speed_threshold_m_per_s {
        0.5 * tr.low_speed_damping_n_s_per_m
            * (1.0 + fm::cos(std::f64::consts::PI * speed / tr.low_speed_threshold_m_per_s))
    } else {
        0.0
    };
    let mut kappa = state.deflection_long_m / sigma_k;
    let mut tan_alpha = state.deflection_lat_m / sigma_a;
    if k_low > 0.0 {
        if ch.kx > 1.0 {
            kappa -= k_low * vsx / ch.kx;
        }
        if ch.ky > 1.0 {
            tan_alpha -= k_low * vsy / ch.ky;
        }
    }
    kappa = clamp(kappa, -tr.slip_ratio_limit, tr.slip_ratio_limit);
    tan_alpha = clamp(tan_alpha, -tr.slip_tangent_limit, tr.slip_tangent_limit);

    op.kappa = kappa;
    op.tan_alpha = tan_alpha;
    let ss = steady_state(p, surface, &op);
    let (px, py) = plough_force(surface, fz, kappa, tan_alpha);

    let crr = p.vertical.rolling_resistance_coefficient + surface.rolling_resistance_add;
    let rr_torque = fz * crr * input.rolling_radius_m;

    out.fx = ss.fx + px;
    out.fy = ss.fy + py;
    out.mz = ss.mz;
    out.rolling_resistance_torque = rr_torque;
    out.slip_ratio = kappa;
    out.slip_angle = fm::atan(tan_alpha);
    out.relaxation_long_m = sigma_k;
    out.relaxation_lat_m = sigma_a;
    out.characteristics = ss.characteristics;
    out.pneumatic_trail = ss.pneumatic_trail;
    // Power leaving the vehicle through the contact patch. Positive when
    // the forces oppose the slip velocities, which is the dissipative case.
    out.sliding_power_w = (-(out.fx * vsx + out.fy * vsy)).max(0.0);
    // Wear in proportion to the sliding work.
    state.wear = (state.wear + p.wear.rate_per_mj * out.sliding_power_w * dt * 1e-6).min(1.0);

    state.thermal.step(
        &p.thermal,
        &ThermalInput {
            sliding_power_w: out.sliding_power_w,
            rolling_power_w: rr_torque * input.omega.abs(),
            speed_m_per_s: speed,
            in_contact: true,
            air_temp_c: input.air_temp_c,
            road_temp_c: input.road_temp_c,
            tread_scale,
        },
        dt,
    );
    out
}
