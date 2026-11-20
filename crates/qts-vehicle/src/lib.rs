// SPDX-License-Identifier: Apache-2.0
//! Vehicle model.
//!
//! - [`car`]: the sprung body, unsprung masses, suspension, steering,
//!   brakes and aerodynamics, and the per-tick [`car::Car::step`].
//! - [`drivetrain`]: swappable drivetrain modules behind the
//!   [`drivetrain::Drivetrain`] trait.
//! - [`spin`]: the solver for wheels, engine and motors coupled by friction
//!   elements.
//! - [`ground`]: the query the car makes of the world.
//! - [`params`]: parameter structs mirroring `data/cars`.
//!
//! The shipped cars are fictional and their parameters are placeholders.

pub mod car;
pub mod drivetrain;
pub mod ground;
pub mod params;
pub mod spin;

pub use car::{Assists, Car, CarOutput, CarState, DriverControls, Environment, WheelOutput};
pub use ground::{FlatGround, Ground, GroundSample};
pub use params::CarParams;

/// Parameter sets for unit tests in this and other crates. Round numbers,
/// not data for any real vehicle.
#[doc(hidden)]
pub mod test_support {
    use crate::params::*;

    fn axle(front: bool) -> AxleParams {
        AxleParams {
            track_m: 1.5,
            unsprung_mass_kg: if front { 38.0 } else { 32.0 },
            wheel_inertia_kg_m2: 1.1,
            spring_rate_n_per_m: if front { 32_000.0 } else { 28_000.0 },
            motion_ratio: 1.0,
            damper_bump_n_s_per_m: 2600.0,
            damper_rebound_n_s_per_m: 3800.0,
            anti_roll_bar_n_per_m: if front { 12_000.0 } else { 9000.0 },
            bump_travel_m: 0.08,
            droop_travel_m: 0.09,
            bump_stop_rate_n_per_m: 150_000.0,
            static_camber_rad: -0.015,
            camber_gain_rad_per_m: -0.4,
            static_toe_rad: 0.0,
            roll_center_height_m: if front { 0.05 } else { 0.12 },
            anti_pitch_tan: 0.05,
        }
    }

    fn body() -> BodyParams {
        BodyParams {
            sprung_mass_kg: 1150.0,
            inertia_roll_kg_m2: 380.0,
            inertia_pitch_kg_m2: 1500.0,
            inertia_yaw_kg_m2: 1700.0,
            wheelbase_m: 2.6,
            cg_to_front_axle_m: 1.0,
            cg_height_m: 0.55,
            length_m: 4.1,
            width_m: 1.75,
            height_m: 1.2,
            ground_clearance_m: 0.13,
            contact_stiffness_n_per_m: 200_000.0,
            contact_damping_n_s_per_m: 8000.0,
            contact_friction: 0.5,
        }
    }

    fn common(drivetrain: DrivetrainParams, id: &str) -> CarParams {
        CarParams {
            id: id.into(),
            name: format!("Test {id}"),
            class: "test".into(),
            description: String::new(),
            tire_front: "test".into(),
            tire_rear: "test".into(),
            body: body(),
            suspension: SuspensionParams {
                front: axle(true),
                rear: axle(false),
            },
            steering: SteeringParams {
                ratio: 14.0,
                max_wheel_angle_rad: 7.85,
                ackermann_fraction: 0.6,
                caster_rad: 0.09,
                kingpin_inclination_rad: 0.2,
                scrub_radius_m: 0.01,
                mechanical_trail_m: 0.02,
                compliance_rad_per_nm: 0.0,
                assist_speed_kmh: vec![0.0],
                assist_gain: vec![0.0],
                assist_threshold_nm: 0.0,
                rack_friction_nm: 0.0,
                steering_inertia_kg_m2: 0.0,
            },
            brakes: BrakeParams {
                max_torque_front_nm: 2600.0,
                max_torque_rear_nm: 1200.0,
                handbrake_torque_nm: 1500.0,
                thermal_front: BrakeThermalParams {
                    heat_capacity_j_per_k: 4000.0,
                    disc_heat_share: 0.9,
                    convection_w_per_k: 6.0,
                    convection_speed_gain: 0.8,
                    radiation_area_m2: 0.07,
                    duct_cooling_gain: 1.0,
                    duct_opening: 0.0,
                },
                thermal_rear: BrakeThermalParams {
                    heat_capacity_j_per_k: 2200.0,
                    disc_heat_share: 0.9,
                    convection_w_per_k: 4.0,
                    convection_speed_gain: 0.5,
                    radiation_area_m2: 0.05,
                    duct_cooling_gain: 1.0,
                    duct_opening: 0.0,
                },
                fade_temp_c: vec![0.0, 100.0, 350.0, 500.0, 650.0, 800.0],
                fade_friction_factor: vec![0.92, 1.0, 1.0, 0.85, 0.6, 0.45],
                abs: AbsParams {
                    enabled: true,
                    slip_threshold: 0.14,
                    release_rate_per_s: 12.0,
                    apply_rate_per_s: 6.0,
                    min_speed_m_per_s: 2.0,
                    control_gain_per_s: 80.0,
                },
            },
            aero: AeroParams {
                air_density_kg_per_m3: 1.2,
                drag_area_m2: 0.7,
                lift_area_front_m2: 0.05,
                lift_area_rear_m2: 0.08,
            },
            drivetrain,
            traction_control: TractionControlParams::default(),
        }
    }

    /// Front-drive car with an engine and manual gearbox.
    pub fn car_engine_manual(limited_slip: bool) -> CarParams {
        common(
            DrivetrainParams::EngineManual(EngineManualParams {
                driven_axle: DrivenAxle::Front,
                engine: EngineParams {
                    torque_curve_rpm: vec![1000.0, 2000.0, 4500.0, 6000.0, 6800.0],
                    torque_curve_nm: vec![180.0, 280.0, 280.0, 230.0, 180.0],
                    inertia_kg_m2: 0.18,
                    idle_rpm: 850.0,
                    redline_rpm: 6600.0,
                    friction_torque_nm: 12.0,
                    friction_nm_per_rad_s: 0.03,
                    overrev_damage_rpm: 7300.0,
                    overrev_failure_rpm: 8800.0,
                },
                clutch: ClutchParams {
                    max_torque_nm: 420.0,
                    auto_engage_start_rpm: 1100.0,
                    auto_engage_full_rpm: 2000.0,
                    free_travel: 0.08,
                    bite_point: 0.45,
                    bite_width: 0.25,
                },
                gearbox: GearboxParams {
                    ratios: vec![3.3, 2.0, 1.4, 1.05, 0.85, 0.7],
                    reverse_ratio: 3.2,
                    final_drive: 3.9,
                    shift_time_s: 0.25,
                },
                differential: DifferentialParams {
                    kind: if limited_slip {
                        DifferentialKind::LimitedSlip
                    } else {
                        DifferentialKind::Open
                    },
                    preload_nm: 40.0,
                    lock_fraction_drive: 0.35,
                    lock_fraction_coast: 0.15,
                },
            }),
            "engine_manual",
        )
    }

    /// Dual-motor electric car.
    pub fn car_dual_motor() -> CarParams {
        let motor = |torque: f64, power: f64| MotorParams {
            peak_torque_nm: torque,
            peak_power_w: power,
            max_speed_rad_per_s: 1800.0,
            reduction_ratio: 9.0,
            rotor_inertia_kg_m2: 0.05,
            efficiency: 0.9,
            regen_torque_limit_nm: 0.5 * torque,
            regen_power_limit_w: 0.4 * power,
        };
        let mut p = common(
            DrivetrainParams::DualMotorElectric(DualMotorParams {
                front_motor: motor(220.0, 130_000.0),
                rear_motor: motor(330.0, 200_000.0),
                front_torque_share: 0.4,
                battery: BatteryParams {
                    capacity_j: 2.7e8,
                    initial_soc: 0.8,
                    max_discharge_power_w: 350_000.0,
                    max_charge_power_w: 120_000.0,
                },
                regen: RegenParams {
                    coast_fraction: 0.15,
                    brake_share: 0.7,
                    fade_low_m_per_s: 1.5,
                    fade_high_m_per_s: 4.0,
                },
                torque_vectoring: TorqueVectoringKind::None,
            }),
            "dual_motor",
        );
        p.body.sprung_mass_kg = 1650.0;
        p.body.cg_to_front_axle_m = 1.3;
        p.body.cg_height_m = 0.48;
        p.suspension.front.spring_rate_n_per_m = 42_000.0;
        p.suspension.rear.spring_rate_n_per_m = 46_000.0;
        p.brakes.max_torque_front_nm = 3400.0;
        p.brakes.max_torque_rear_nm = 2000.0;
        p
    }
}
