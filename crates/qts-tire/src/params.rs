// SPDX-License-Identifier: Apache-2.0
//! Tire and surface parameter sets.
//!
//! These structs mirror the data files in `data/tires` and `data/surfaces`
//! after the loader has stripped each `{ value, source, confidence }` table
//! down to its value. Nothing in this crate supplies a default: a missing
//! parameter is a load error.

use serde::Deserialize;

/// All parameters of one tire.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TireParams {
    /// Identifier, equal to the data file stem.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Free-text description.
    #[serde(default)]
    pub description: String,
    /// Size.
    pub geometry: TireGeometry,
    /// Vertical behaviour and rolling resistance.
    pub vertical: TireVertical,
    /// Steady-state Magic Formula coefficients.
    pub magic_formula: MagicFormulaParams,
    /// Relaxation behaviour.
    pub transient: TransientParams,
    /// Temperature model.
    pub thermal: ThermalParams,
    /// Inflation pressure model.
    pub pressure: PressureParams,
    /// Tread wear.
    pub wear: WearParams,
}

/// Tread wear, driven by the energy dissipated by sliding in the contact
/// patch (an Archard-type law: material removed in proportion to friction
/// work).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WearParams {
    /// Share of the usable tread removed per megajoule of sliding energy.
    pub rate_per_mj: f64,
    /// Fractional loss of friction at full wear, standing for the compound
    /// ageing and graining that come with use. A shallower tread alone
    /// grips as well or better in the dry; this is the degradation, not
    /// the depth.
    pub grip_loss_at_full_wear: f64,
    /// Fractional gain of slip and cornering stiffness at full wear: less
    /// tread rubber to squirm.
    pub stiffness_gain_at_full_wear: f64,
    /// Share of the tread surface and core heat capacity gone at full
    /// wear: a worn tread heats and cools faster.
    pub tread_heat_capacity_share: f64,
}

/// Tire size.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TireGeometry {
    /// Free radius with no load, m.
    pub unloaded_radius_m: f64,
    /// Section width, m. Used for drawing and for the contact estimate in
    /// the thermal model.
    pub width_m: f64,
}

/// Vertical behaviour and rolling resistance.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TireVertical {
    /// Radial stiffness at nominal pressure, N/m.
    pub stiffness_n_per_m: f64,
    /// Radial damping, N·s/m.
    pub damping_n_s_per_m: f64,
    /// Nominal load `Fz0` the Magic Formula coefficients refer to, N.
    pub nominal_load_n: f64,
    /// Nominal inflation pressure the coefficients refer to, Pa (absolute).
    pub nominal_pressure_pa: f64,
    /// Rolling resistance coefficient on a hard surface (force per load).
    pub rolling_resistance_coefficient: f64,
}

/// Magic Formula coefficients.
///
/// This is a subset of MF 5.2 / PAC2002. Field names are the PAC2002
/// coefficient names in lower case. Coefficients that are not listed here
/// (horizontal and vertical shifts, ply-steer, conicity, turn slip) are not
/// implemented and behave as zero.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MagicFormulaParams {
    /// Longitudinal shape factor `Cx`.
    pub pcx1: f64,
    /// Longitudinal friction `μx` at nominal load.
    pub pdx1: f64,
    /// Variation of `μx` with load.
    pub pdx2: f64,
    /// Longitudinal curvature `Ex` at nominal load.
    pub pex1: f64,
    /// Variation of `Ex` with load.
    pub pex2: f64,
    /// Longitudinal slip stiffness `Kx / Fz` at nominal load.
    pub pkx1: f64,
    /// Variation of slip stiffness with load.
    pub pkx2: f64,
    /// Exponent in the slip stiffness load dependence.
    pub pkx3: f64,

    /// Lateral shape factor `Cy`.
    pub pcy1: f64,
    /// Lateral friction `μy` at nominal load.
    pub pdy1: f64,
    /// Variation of `μy` with load.
    pub pdy2: f64,
    /// Variation of `μy` with squared camber.
    pub pdy3: f64,
    /// Lateral curvature `Ey` at nominal load.
    pub pey1: f64,
    /// Variation of `Ey` with load.
    pub pey2: f64,
    /// Maximum cornering stiffness `Ky / Fz0`.
    pub pky1: f64,
    /// Load, as a multiple of `Fz0`, at which cornering stiffness peaks.
    pub pky2: f64,
    /// Variation of cornering stiffness with camber.
    pub pky3: f64,
    /// Camber thrust per unit load per radian of camber.
    pub pvy3: f64,

    /// Slope factor for the reduction of `Fx` with slip angle.
    pub rbx1: f64,
    /// Variation of that slope with slip ratio.
    pub rbx2: f64,
    /// Shape factor for the reduction of `Fx` with slip angle.
    pub rcx1: f64,
    /// Slope factor for the reduction of `Fy` with slip ratio.
    pub rby1: f64,
    /// Variation of that slope with slip angle.
    pub rby2: f64,
    /// Slip-angle shift in that slope.
    pub rby3: f64,
    /// Shape factor for the reduction of `Fy` with slip ratio.
    pub rcy1: f64,

    /// Pneumatic trail slope factor `Bt` at nominal load.
    pub qbz1: f64,
    /// Variation of `Bt` with load.
    pub qbz2: f64,
    /// Pneumatic trail shape factor `Ct`.
    pub qcz1: f64,
    /// Peak pneumatic trail as a fraction of the unloaded radius at nominal
    /// load.
    pub qdz1: f64,
    /// Variation of peak trail with load.
    pub qdz2: f64,
    /// Pneumatic trail curvature `Et`.
    pub qez1: f64,
}

/// Relaxation behaviour.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TransientParams {
    /// Longitudinal carcass stiffness at nominal pressure, N/m. The
    /// longitudinal relaxation length is `Kx / stiffness`.
    pub carcass_stiffness_long_n_per_m: f64,
    /// Lateral carcass stiffness at nominal pressure, N/m. The lateral
    /// relaxation length is `Ky / stiffness`.
    pub carcass_stiffness_lat_n_per_m: f64,
    /// Lower bound on either relaxation length, m.
    pub min_relaxation_length_m: f64,
    /// Damping added to the slip input at low speed, N·s/m.
    pub low_speed_damping_n_s_per_m: f64,
    /// Speed below which the low-speed damping fades in, m/s.
    pub low_speed_threshold_m_per_s: f64,
    /// Limit on the magnitude of transient slip ratio (anti-windup).
    pub slip_ratio_limit: f64,
    /// Limit on the tangent of transient slip angle (anti-windup).
    pub slip_tangent_limit: f64,
}

/// Temperature model: three tire nodes and the inflation gas.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ThermalParams {
    /// Heat capacity of the tread surface layer, J/K.
    pub surface_heat_capacity_j_per_k: f64,
    /// Heat capacity of the tread bulk, J/K.
    pub core_heat_capacity_j_per_k: f64,
    /// Heat capacity of the carcass, J/K.
    pub carcass_heat_capacity_j_per_k: f64,
    /// Heat capacity of the inflation gas, J/K.
    pub gas_heat_capacity_j_per_k: f64,
    /// Conductance between surface and core, W/K.
    pub conductance_surface_core_w_per_k: f64,
    /// Conductance between core and carcass, W/K.
    pub conductance_core_carcass_w_per_k: f64,
    /// Conductance between carcass and gas, W/K.
    pub conductance_carcass_gas_w_per_k: f64,
    /// Conductance between the tread surface and the road while loaded, W/K.
    pub conductance_surface_road_w_per_k: f64,
    /// Convection from the surface to air at rest, W/K.
    pub convection_surface_w_per_k: f64,
    /// Extra surface convection per m/s of speed, W/K per m/s.
    pub convection_surface_speed_gain: f64,
    /// Convection from the carcass (sidewalls) to air at rest, W/K.
    pub convection_carcass_w_per_k: f64,
    /// Extra carcass convection per m/s of speed, W/K per m/s.
    pub convection_carcass_speed_gain: f64,
    /// Fraction of sliding power that heats the tire surface; the rest goes
    /// into the road.
    pub friction_heat_fraction: f64,
    /// Fraction of rolling-loss power deposited in the core; the rest heats
    /// the carcass.
    pub hysteresis_core_fraction: f64,
    /// Temperature of best grip, °C.
    pub optimal_temp_c: f64,
    /// Width of the grip window, °C: grip follows a Gaussian of this width
    /// around the optimum.
    pub temp_window_c: f64,
    /// Grip multiplier far from the optimum.
    pub min_grip_factor: f64,
    /// Weight of the surface node in the grip temperature; the core gets
    /// the rest.
    pub surface_weight: f64,
    /// Fractional loss of slip and cornering stiffness per kelvin of carcass
    /// temperature above the reference.
    pub stiffness_temp_sensitivity_per_k: f64,
    /// Carcass temperature at which the stiffness coefficients apply, °C.
    pub reference_temp_c: f64,
}

/// Inflation pressure model. Sensitivity names follow MF 6.1 where one
/// exists; all act on `dpi = (p − p_nominal) / p_nominal`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PressureParams {
    /// Cold inflation pressure, Pa (absolute).
    pub cold_pressure_pa: f64,
    /// Gas temperature at which the cold pressure was set, °C.
    pub cold_temp_c: f64,
    /// Linear effect of pressure on slip stiffness.
    pub ppx1: f64,
    /// Linear effect of pressure on longitudinal friction.
    pub ppx3: f64,
    /// Quadratic effect of pressure on longitudinal friction.
    pub ppx4: f64,
    /// Linear effect of pressure on peak cornering stiffness.
    pub ppy1: f64,
    /// Effect of pressure on the load at which cornering stiffness peaks.
    pub ppy2: f64,
    /// Linear effect of pressure on lateral friction.
    pub ppy3: f64,
    /// Quadratic effect of pressure on lateral friction.
    pub ppy4: f64,
    /// Linear effect of pressure on vertical stiffness.
    pub pfz1: f64,
    /// Linear effect of pressure on both carcass stiffnesses.
    pub carcass_stiffness_sensitivity: f64,
}

/// Parameters of one road surface, as seen by the tire.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SurfaceParams {
    /// Identifier, equal to the data file stem.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Free-text description.
    #[serde(default)]
    pub description: String,
    /// False while the surface is a first-pass model that has not been
    /// tuned against anything. Shown in the UI and reports.
    pub tuned: bool,
    /// Multiplier on peak friction in both directions.
    pub friction_scale: f64,
    /// Multiplier on slip and cornering stiffness. Below 1 moves the force
    /// peak to larger slip, as on loose surfaces.
    pub stiffness_scale: f64,
    /// How much of the tire's post-peak force drop remains, 0 to 1. At 1 the
    /// tire's own shape factors apply; at 0 force rises to the peak and
    /// stays there.
    pub falloff: f64,
    /// Extra rolling resistance coefficient from surface deformation.
    pub rolling_resistance_add: f64,
    /// Ploughing force per unit load at full slip: material displaced by a
    /// sliding tire on a loose surface adds force that grows with slip.
    pub plough_coefficient: f64,
    /// Amplitude of the roughness height profile, m.
    pub roughness_amplitude_m: f64,
    /// Dominant wavelength of the roughness, m.
    pub roughness_wavelength_m: f64,
}

impl TireParams {
    /// Checks that values are inside the ranges the model assumes. Returns
    /// a description of the first problem found.
    pub fn validate(&self) -> Result<(), String> {
        fn positive(name: &str, v: f64) -> Result<(), String> {
            if v.is_finite() && v > 0.0 {
                Ok(())
            } else {
                Err(format!("{name} must be positive and finite, got {v}"))
            }
        }
        fn unit(name: &str, v: f64) -> Result<(), String> {
            if v.is_finite() && (0.0..=1.0).contains(&v) {
                Ok(())
            } else {
                Err(format!("{name} must be between 0 and 1, got {v}"))
            }
        }
        positive(
            "geometry.unloaded_radius_m",
            self.geometry.unloaded_radius_m,
        )?;
        positive("geometry.width_m", self.geometry.width_m)?;
        positive(
            "vertical.stiffness_n_per_m",
            self.vertical.stiffness_n_per_m,
        )?;
        positive("vertical.nominal_load_n", self.vertical.nominal_load_n)?;
        positive(
            "vertical.nominal_pressure_pa",
            self.vertical.nominal_pressure_pa,
        )?;
        let mf = &self.magic_formula;
        positive("magic_formula.pcx1", mf.pcx1)?;
        positive("magic_formula.pdx1", mf.pdx1)?;
        positive("magic_formula.pkx1", mf.pkx1)?;
        positive("magic_formula.pcy1", mf.pcy1)?;
        positive("magic_formula.pdy1", mf.pdy1)?;
        positive("magic_formula.pky1", mf.pky1)?;
        positive("magic_formula.pky2", mf.pky2)?;
        let tr = &self.transient;
        positive(
            "transient.carcass_stiffness_long_n_per_m",
            tr.carcass_stiffness_long_n_per_m,
        )?;
        positive(
            "transient.carcass_stiffness_lat_n_per_m",
            tr.carcass_stiffness_lat_n_per_m,
        )?;
        positive(
            "transient.min_relaxation_length_m",
            tr.min_relaxation_length_m,
        )?;
        positive(
            "transient.low_speed_threshold_m_per_s",
            tr.low_speed_threshold_m_per_s,
        )?;
        positive("transient.slip_ratio_limit", tr.slip_ratio_limit)?;
        positive("transient.slip_tangent_limit", tr.slip_tangent_limit)?;
        let th = &self.thermal;
        positive(
            "thermal.surface_heat_capacity_j_per_k",
            th.surface_heat_capacity_j_per_k,
        )?;
        positive(
            "thermal.core_heat_capacity_j_per_k",
            th.core_heat_capacity_j_per_k,
        )?;
        positive(
            "thermal.carcass_heat_capacity_j_per_k",
            th.carcass_heat_capacity_j_per_k,
        )?;
        positive(
            "thermal.gas_heat_capacity_j_per_k",
            th.gas_heat_capacity_j_per_k,
        )?;
        positive("thermal.temp_window_c", th.temp_window_c)?;
        unit("thermal.friction_heat_fraction", th.friction_heat_fraction)?;
        unit(
            "thermal.hysteresis_core_fraction",
            th.hysteresis_core_fraction,
        )?;
        unit("thermal.min_grip_factor", th.min_grip_factor)?;
        unit("thermal.surface_weight", th.surface_weight)?;
        positive("pressure.cold_pressure_pa", self.pressure.cold_pressure_pa)?;
        let w = &self.wear;
        positive("wear.rate_per_mj", w.rate_per_mj)?;
        unit("wear.grip_loss_at_full_wear", w.grip_loss_at_full_wear)?;
        unit(
            "wear.stiffness_gain_at_full_wear",
            w.stiffness_gain_at_full_wear,
        )?;
        if !(0.0..0.95).contains(&w.tread_heat_capacity_share) {
            return Err("wear.tread_heat_capacity_share must be in [0, 0.95)".into());
        }
        Ok(())
    }
}

impl SurfaceParams {
    /// Checks that values are inside the ranges the model assumes.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.friction_scale.is_finite() && self.friction_scale > 0.0) {
            return Err(format!(
                "friction_scale must be positive, got {}",
                self.friction_scale
            ));
        }
        if !(self.stiffness_scale.is_finite() && self.stiffness_scale > 0.0) {
            return Err(format!(
                "stiffness_scale must be positive, got {}",
                self.stiffness_scale
            ));
        }
        if !(0.0..=1.0).contains(&self.falloff) {
            return Err(format!(
                "falloff must be between 0 and 1, got {}",
                self.falloff
            ));
        }
        if !(self.roughness_wavelength_m.is_finite() && self.roughness_wavelength_m > 0.0) {
            return Err(format!(
                "roughness_wavelength_m must be positive, got {}",
                self.roughness_wavelength_m
            ));
        }
        for (name, v) in [
            ("rolling_resistance_add", self.rolling_resistance_add),
            ("plough_coefficient", self.plough_coefficient),
            ("roughness_amplitude_m", self.roughness_amplitude_m),
        ] {
            if !(v.is_finite() && v >= 0.0) {
                return Err(format!("{name} must be zero or positive, got {v}"));
            }
        }
        Ok(())
    }
}
