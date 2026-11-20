// SPDX-License-Identifier: Apache-2.0
//! Tire model.
//!
//! Four layers, each usable alone:
//!
//! 1. [`magic`]: steady-state forces from a documented subset of the Magic
//!    Formula.
//! 2. [`model`]: the transient layer (carcass deflection and relaxation
//!    length) and the per-tick [`model::step`] that ties everything together.
//! 3. [`thermal`]: surface, core, carcass and gas temperatures, grip window
//!    and inflation pressure.
//! 4. [`params::SurfaceParams`]: what the road surface contributes.
//!
//! Every number comes from a data file through [`params`]. The shipped
//! parameter sets are placeholders; nothing in this crate is validated
//! against a real tire.

pub mod magic;
pub mod model;
pub mod params;
pub mod thermal;

pub use magic::{steady_state, Characteristics, Operating, SteadyForces};
pub use model::{step, vertical_load, vertical_stiffness, TireInput, TireOutput, TireState};
pub use params::{SurfaceParams, TireParams};
pub use thermal::ThermalState;

/// Parameter sets used by this crate's tests and by other crates' unit
/// tests. They are arbitrary round numbers, not data for any real tire.
#[doc(hidden)]
pub mod test_support {
    use crate::params::*;

    /// A tire with round-number coefficients.
    pub fn tire() -> TireParams {
        TireParams {
            id: "test".into(),
            name: "Test tire".into(),
            description: String::new(),
            geometry: TireGeometry {
                unloaded_radius_m: 0.31,
                width_m: 0.215,
            },
            vertical: TireVertical {
                stiffness_n_per_m: 220_000.0,
                damping_n_s_per_m: 500.0,
                nominal_load_n: 4000.0,
                nominal_pressure_pa: 330_000.0,
                rolling_resistance_coefficient: 0.012,
            },
            magic_formula: MagicFormulaParams {
                pcx1: 1.6,
                pdx1: 1.1,
                pdx2: -0.1,
                pex1: 0.2,
                pex2: 0.0,
                pkx1: 20.0,
                pkx2: 0.0,
                pkx3: 0.2,
                pcy1: 1.4,
                pdy1: 1.0,
                pdy2: -0.15,
                pdy3: 2.0,
                pey1: -0.5,
                pey2: 0.0,
                pky1: 18.0,
                pky2: 2.0,
                pky3: 0.3,
                pvy3: 1.0,
                rbx1: 12.0,
                rbx2: 10.0,
                rcx1: 1.0,
                rby1: 10.0,
                rby2: 8.0,
                rby3: 0.0,
                rcy1: 1.0,
                qbz1: 10.0,
                qbz2: -1.0,
                qcz1: 1.1,
                qdz1: 0.1,
                qdz2: -0.01,
                qez1: -1.5,
            },
            transient: TransientParams {
                carcass_stiffness_long_n_per_m: 450_000.0,
                carcass_stiffness_lat_n_per_m: 150_000.0,
                min_relaxation_length_m: 0.02,
                low_speed_damping_n_s_per_m: 3000.0,
                low_speed_threshold_m_per_s: 2.5,
                slip_ratio_limit: 2.0,
                slip_tangent_limit: 5.0,
            },
            thermal: ThermalParams {
                surface_heat_capacity_j_per_k: 800.0,
                core_heat_capacity_j_per_k: 6000.0,
                carcass_heat_capacity_j_per_k: 9000.0,
                gas_heat_capacity_j_per_k: 100.0,
                conductance_surface_core_w_per_k: 120.0,
                conductance_core_carcass_w_per_k: 60.0,
                conductance_carcass_gas_w_per_k: 15.0,
                conductance_surface_road_w_per_k: 40.0,
                convection_surface_w_per_k: 8.0,
                convection_surface_speed_gain: 2.0,
                convection_carcass_w_per_k: 6.0,
                convection_carcass_speed_gain: 0.6,
                friction_heat_fraction: 0.6,
                hysteresis_core_fraction: 0.5,
                optimal_temp_c: 60.0,
                temp_window_c: 50.0,
                min_grip_factor: 0.85,
                surface_weight: 0.6,
                stiffness_temp_sensitivity_per_k: 0.002,
                reference_temp_c: 40.0,
            },
            pressure: PressureParams {
                cold_pressure_pa: 330_000.0,
                cold_temp_c: 20.0,
                ppx1: -0.3,
                ppx3: -0.1,
                ppx4: 0.0,
                ppy1: 0.4,
                ppy2: 1.3,
                ppy3: -0.1,
                ppy4: 0.0,
                pfz1: 0.7,
                carcass_stiffness_sensitivity: 0.5,
            },
            wear: WearParams {
                rate_per_mj: 0.002,
                grip_loss_at_full_wear: 0.1,
                stiffness_gain_at_full_wear: 0.15,
                tread_heat_capacity_share: 0.5,
            },
        }
    }

    /// A hard surface that changes nothing.
    pub fn tarmac() -> SurfaceParams {
        SurfaceParams {
            id: "tarmac".into(),
            name: "Tarmac".into(),
            description: String::new(),
            tuned: true,
            friction_scale: 1.0,
            stiffness_scale: 1.0,
            falloff: 1.0,
            rolling_resistance_add: 0.0,
            plough_coefficient: 0.0,
            roughness_amplitude_m: 0.0,
            roughness_wavelength_m: 1.0,
        }
    }

    /// A loose surface.
    pub fn gravel() -> SurfaceParams {
        SurfaceParams {
            id: "gravel".into(),
            name: "Gravel".into(),
            description: String::new(),
            tuned: false,
            friction_scale: 0.6,
            stiffness_scale: 0.5,
            falloff: 0.2,
            rolling_resistance_add: 0.03,
            plough_coefficient: 0.12,
            roughness_amplitude_m: 0.008,
            roughness_wavelength_m: 0.6,
        }
    }
}
