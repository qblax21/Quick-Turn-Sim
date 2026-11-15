// SPDX-License-Identifier: Apache-2.0
//! Temperature and inflation pressure.
//!
//! Three lumped tire nodes (tread surface, tread core, carcass) and the
//! inflation gas exchange heat with each other, the road and the air.
//! Sliding power heats the surface; rolling losses heat the core and
//! carcass. Grip follows a Gaussian window around an optimum temperature,
//! and pressure follows the gas temperature at constant volume.

use crate::params::{ThermalParams, TireParams};
use qts_core::math::fm;
use qts_core::snapshot::{SnapshotError, StateReader, StateWriter};

const KELVIN_OFFSET: f64 = 273.15;

/// Temperatures of one tire, °C.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThermalState {
    /// Tread surface.
    pub surface_c: f64,
    /// Tread bulk.
    pub core_c: f64,
    /// Carcass.
    pub carcass_c: f64,
    /// Inflation gas.
    pub gas_c: f64,
}

/// Heat sources and surroundings for one thermal step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThermalInput {
    /// Power dissipated by sliding in the contact patch, W.
    pub sliding_power_w: f64,
    /// Power dissipated by rolling resistance, W.
    pub rolling_power_w: f64,
    /// Speed of the tire through the air, m/s.
    pub speed_m_per_s: f64,
    /// True while the tire carries load and touches the road.
    pub in_contact: bool,
    /// Air temperature, °C.
    pub air_temp_c: f64,
    /// Road surface temperature, °C.
    pub road_temp_c: f64,
}

impl ThermalState {
    /// All nodes at one temperature.
    pub fn uniform(temp_c: f64) -> Self {
        Self {
            surface_c: temp_c,
            core_c: temp_c,
            carcass_c: temp_c,
            gas_c: temp_c,
        }
    }

    /// Advances the temperatures by `dt` seconds (explicit Euler; every time
    /// constant in the model is seconds, far above the tick length).
    pub fn step(&mut self, p: &ThermalParams, input: &ThermalInput, dt: f64) {
        let v = input.speed_m_per_s.abs();
        let q_sc = p.conductance_surface_core_w_per_k * (self.surface_c - self.core_c);
        let q_cc = p.conductance_core_carcass_w_per_k * (self.core_c - self.carcass_c);
        let q_cg = p.conductance_carcass_gas_w_per_k * (self.carcass_c - self.gas_c);
        let q_road = if input.in_contact {
            p.conductance_surface_road_w_per_k * (self.surface_c - input.road_temp_c)
        } else {
            0.0
        };
        let q_air_surface = (p.convection_surface_w_per_k + p.convection_surface_speed_gain * v)
            * (self.surface_c - input.air_temp_c);
        let q_air_carcass = (p.convection_carcass_w_per_k + p.convection_carcass_speed_gain * v)
            * (self.carcass_c - input.air_temp_c);

        let heat_surface = input.sliding_power_w.max(0.0) * p.friction_heat_fraction;
        let rolling = input.rolling_power_w.max(0.0);
        let heat_core = rolling * p.hysteresis_core_fraction;
        let heat_carcass = rolling * (1.0 - p.hysteresis_core_fraction);

        self.surface_c +=
            dt * (heat_surface - q_sc - q_road - q_air_surface) / p.surface_heat_capacity_j_per_k;
        self.core_c += dt * (heat_core + q_sc - q_cc) / p.core_heat_capacity_j_per_k;
        self.carcass_c +=
            dt * (heat_carcass + q_cc - q_cg - q_air_carcass) / p.carcass_heat_capacity_j_per_k;
        self.gas_c += dt * q_cg / p.gas_heat_capacity_j_per_k;
    }

    /// Friction multiplier from temperature: 1 at the optimum, falling to
    /// `min_grip_factor` far from it.
    pub fn grip_factor(&self, p: &ThermalParams) -> f64 {
        let t = p.surface_weight * self.surface_c + (1.0 - p.surface_weight) * self.core_c;
        let x = (t - p.optimal_temp_c) / p.temp_window_c;
        p.min_grip_factor + (1.0 - p.min_grip_factor) * fm::exp(-x * x)
    }

    /// Stiffness multiplier from carcass temperature, bounded to
    /// `[0.5, 1.5]`.
    pub fn stiffness_factor(&self, p: &ThermalParams) -> f64 {
        let f = 1.0 - p.stiffness_temp_sensitivity_per_k * (self.carcass_c - p.reference_temp_c);
        f.clamp(0.5, 1.5)
    }

    /// Absolute inflation pressure at the current gas temperature, Pa.
    pub fn pressure_pa(&self, tire: &TireParams) -> f64 {
        let pr = &tire.pressure;
        pr.cold_pressure_pa * (self.gas_c + KELVIN_OFFSET) / (pr.cold_temp_c + KELVIN_OFFSET)
    }

    /// Relative pressure deviation from nominal.
    pub fn dpi(&self, tire: &TireParams) -> f64 {
        let nominal = tire.vertical.nominal_pressure_pa;
        (self.pressure_pa(tire) - nominal) / nominal
    }

    /// True when every temperature is finite.
    pub fn is_finite(&self) -> bool {
        self.surface_c.is_finite()
            && self.core_c.is_finite()
            && self.carcass_c.is_finite()
            && self.gas_c.is_finite()
    }

    pub(crate) fn save(&self, w: &mut StateWriter) {
        w.write_f64(self.surface_c);
        w.write_f64(self.core_c);
        w.write_f64(self.carcass_c);
        w.write_f64(self.gas_c);
    }

    pub(crate) fn load(&mut self, r: &mut StateReader<'_>) -> Result<(), SnapshotError> {
        self.surface_c = r.read_f64()?;
        self.core_c = r.read_f64()?;
        self.carcass_c = r.read_f64()?;
        self.gas_c = r.read_f64()?;
        Ok(())
    }
}
