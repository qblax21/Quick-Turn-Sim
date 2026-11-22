// SPDX-License-Identifier: Apache-2.0
//! Drivetrains as swappable modules.
//!
//! A drivetrain owns everything that spins besides the tires' contact with
//! the road: it takes the driver's controls, the torques the tires and
//! brakes put on the wheels, and advances the four wheel speeds together
//! with its own rotating parts.

mod electric;
mod engine_manual;

pub use electric::{DualMotor, FixedSplit, TorqueVectoring, TvCommand, TvContext};
pub use engine_manual::EngineManual;

use crate::params::DrivetrainParams;
use qts_core::snapshot::Snapshot;

/// Driver controls relevant to the drivetrain.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriveControls {
    /// Accelerator, 0 to 1.
    pub throttle: f64,
    /// Clutch pedal, 0 (engaged) to 1 (pressed).
    pub clutch: f64,
    /// Sequential upshift requested this tick.
    pub shift_up: bool,
    /// Sequential downshift requested this tick.
    pub shift_down: bool,
    /// Direct gear selection: -1 reverse, 0 neutral, 1.. forward.
    pub gear_request: Option<i8>,
    /// Automatic clutch assist.
    pub auto_clutch: bool,
    /// Automatic gear selection assist.
    pub auto_shift: bool,
    /// Starter motor held.
    pub starter: bool,
    /// Over-revving harms the engine.
    pub engine_damage: bool,
}

/// Everything a drivetrain needs for one tick.
#[derive(Clone, Copy, Debug)]
pub struct DriveContext {
    /// Driver controls.
    pub controls: DriveControls,
    /// Wheel speeds, rad/s (front-left, front-right, rear-left, rear-right).
    pub wheel_omega: [f64; 4],
    /// Wheel spin inertias, kg·m².
    pub wheel_inertia: [f64; 4],
    /// Torque on each wheel from the tire's longitudinal force, N·m.
    pub tire_torque: [f64; 4],
    /// Rolling-resistance torque magnitude per wheel, N·m.
    pub resist_torque: [f64; 4],
    /// Foot-brake torque capacity per wheel after anti-lock modulation, N·m.
    pub service_brake_torque: [f64; 4],
    /// Handbrake torque capacity per wheel, N·m.
    pub handbrake_torque: [f64; 4],
    /// True for wheels where the anti-lock system is intervening.
    pub abs_active: [bool; 4],
    /// Vehicle speed along its own x axis, m/s.
    pub speed: f64,
    /// Yaw rate, rad/s. Offered to the torque-vectoring hook.
    pub yaw_rate: f64,
    /// Steering input, -1 to 1. Offered to the torque-vectoring hook.
    pub steer: f64,
}

/// Drivetrain values for telemetry and the HUD.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriveTelemetry {
    /// Engine speed, or front motor speed for an electric car, rpm.
    pub engine_rpm: f64,
    /// Engine torque at the crank, or total motor torque at the rotors, N·m.
    pub engine_torque_nm: f64,
    /// Selected gear: -1 reverse, 0 neutral, 1.. forward.
    pub gear: i8,
    /// True while a gear change is in progress.
    pub shifting: bool,
    /// Clutch engagement, 0 open to 1 fully engaged.
    pub clutch_engagement: f64,
    /// Torque through the clutch, N·m (zero for an electric car).
    pub clutch_torque_nm: f64,
    /// Rotor torque of the front and rear motors, N·m (zero without motors).
    pub motor_torque_nm: [f64; 2],
    /// Mechanical power delivered to the wheels, W.
    pub drive_power_w: f64,
    /// Mechanical power recovered by regeneration, W.
    pub regen_power_w: f64,
    /// Electrical power drawn from the battery, W. Negative while charging.
    pub battery_power_w: f64,
    /// Battery state of charge, 0 to 1 (zero without a battery).
    pub battery_soc: f64,
    /// True while the engine is stalled.
    pub stalled: bool,
    /// True while a gear is being forced in without the clutch: the box
    /// stays in neutral and grinds.
    pub grinding: bool,
    /// Locking torque available in the differential this tick, N·m (zero
    /// when open).
    pub diff_lock_nm: f64,
    /// Torque the differential moved from one side to the other, N·m.
    pub diff_transfer_nm: f64,
    /// Input torque at the differential case, N·m (negative on overrun).
    pub diff_input_nm: f64,
    /// Difference between engine speed and the speed the requested gear
    /// would turn it at while grinding, rad/s.
    pub grind_speed_rad_s: f64,
    /// Engine speed minus gearbox input speed while the clutch carries
    /// torque, rad/s (zero when locked or open).
    pub clutch_slip_rad_s: f64,
    /// Clutch torque capacity at full engagement, N·m.
    pub clutch_capacity_nm: f64,
    /// Highest engine speed since the car was last reset, rpm.
    pub peak_rpm: f64,
    /// True while the engine is above its over-rev damage speed.
    pub overrev: bool,
    /// Engine damage, 0 (healthy) to 1 (failed).
    pub engine_damage: f64,
}

/// Result of one drivetrain tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriveResult {
    /// Wheel speeds after the tick, rad/s.
    pub wheel_omega: [f64; 4],
    /// Torque each wheel received from the driveline, N·m.
    pub shaft_torque: [f64; 4],
    /// Friction-brake torque applied at each wheel, N·m, signed.
    pub brake_torque: [f64; 4],
    /// Telemetry values.
    pub telemetry: DriveTelemetry,
}

/// A drivetrain module.
pub trait Drivetrain: Snapshot + Send {
    /// Advances the driveline and all four wheels by one tick.
    fn step(&mut self, ctx: &DriveContext, dt: f64) -> DriveResult;

    /// Kinetic energy of the module's own rotating parts, J.
    fn kinetic_energy(&self) -> f64;

    /// Returns the module to its initial state.
    fn reset(&mut self);

    /// Number of forward gears.
    fn forward_gears(&self) -> u8;

    /// Short name of the module type, for reports.
    fn kind_name(&self) -> &'static str;
}

/// Builds the drivetrain module a car's parameters ask for.
pub fn build(params: &DrivetrainParams) -> Box<dyn Drivetrain> {
    match params {
        DrivetrainParams::EngineManual(p) => Box::new(EngineManual::new(p.clone())),
        DrivetrainParams::DualMotorElectric(p) => Box::new(DualMotor::new(p.clone())),
    }
}

/// Conversion from rad/s to rpm.
pub(crate) const RAD_S_TO_RPM: f64 = 30.0 / std::f64::consts::PI;

/// Splits a combined friction impulse back into its brake share.
pub(crate) fn brake_share(total_torque: f64, brake_capacity: f64, resist_capacity: f64) -> f64 {
    let cap = brake_capacity + resist_capacity;
    if cap > 0.0 {
        total_torque * brake_capacity / cap
    } else {
        0.0
    }
}
