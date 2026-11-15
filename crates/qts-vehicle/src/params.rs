// SPDX-License-Identifier: Apache-2.0
//! Vehicle parameter sets, mirroring the files in `data/cars`.
//!
//! As with tires, the loader strips each `{ value, source, confidence }`
//! table to its value before these structs are filled, and nothing here has
//! a default: a missing parameter is a load error.

use serde::Deserialize;

/// All parameters of one car.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CarParams {
    /// Identifier, equal to the data file stem.
    pub id: String,
    /// Display name. Cars are fictional; this is an original name.
    pub name: String,
    /// Vehicle class in plain words.
    pub class: String,
    /// Free-text description.
    #[serde(default)]
    pub description: String,
    /// Identifier of the front tire data file.
    pub tire_front: String,
    /// Identifier of the rear tire data file.
    pub tire_rear: String,
    /// Sprung body.
    pub body: BodyParams,
    /// Suspension, per axle.
    pub suspension: SuspensionParams,
    /// Steering system.
    pub steering: SteeringParams,
    /// Brake system.
    pub brakes: BrakeParams,
    /// Aerodynamics.
    pub aero: AeroParams,
    /// Drivetrain module and its parameters.
    pub drivetrain: DrivetrainParams,
}

/// Sprung body mass properties and outline.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BodyParams {
    /// Sprung mass, kg. Unsprung masses are listed per axle.
    pub sprung_mass_kg: f64,
    /// Roll inertia of the sprung mass about its centre of gravity, kg·m².
    pub inertia_roll_kg_m2: f64,
    /// Pitch inertia of the sprung mass, kg·m².
    pub inertia_pitch_kg_m2: f64,
    /// Yaw inertia of the sprung mass, kg·m².
    pub inertia_yaw_kg_m2: f64,
    /// Distance between axles, m.
    pub wheelbase_m: f64,
    /// Distance from the sprung centre of gravity to the front axle, m.
    pub cg_to_front_axle_m: f64,
    /// Height of the sprung centre of gravity above the ground at design
    /// ride height, m.
    pub cg_height_m: f64,
    /// Overall body length, m. Used for ground contact and drawing.
    pub length_m: f64,
    /// Overall body width, m.
    pub width_m: f64,
    /// Body height from underside to roof, m.
    pub height_m: f64,
    /// Ground clearance of the underside at design ride height, m.
    pub ground_clearance_m: f64,
    /// Stiffness of the body-to-ground penalty contact, N/m per point.
    pub contact_stiffness_n_per_m: f64,
    /// Damping of the body-to-ground contact, N·s/m per point.
    pub contact_damping_n_s_per_m: f64,
    /// Friction coefficient of the body sliding on the ground.
    pub contact_friction: f64,
}

/// Suspension for both axles.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SuspensionParams {
    /// Front axle.
    pub front: AxleParams,
    /// Rear axle.
    pub rear: AxleParams,
}

/// Suspension and wheel parameters for one axle. Values are per corner
/// unless stated.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AxleParams {
    /// Track width, m.
    pub track_m: f64,
    /// Unsprung mass per corner, kg.
    pub unsprung_mass_kg: f64,
    /// Spin inertia of one wheel assembly, kg·m².
    pub wheel_inertia_kg_m2: f64,
    /// Spring rate at the spring, N/m.
    pub spring_rate_n_per_m: f64,
    /// Spring and damper travel per unit wheel travel.
    pub motion_ratio: f64,
    /// Damper rate in bump at the damper, N·s/m.
    pub damper_bump_n_s_per_m: f64,
    /// Damper rate in rebound at the damper, N·s/m.
    pub damper_rebound_n_s_per_m: f64,
    /// Anti-roll bar rate at the wheel: force per metre of left-right
    /// travel difference, N/m.
    pub anti_roll_bar_n_per_m: f64,
    /// Wheel travel in bump before the bump stop engages, m.
    pub bump_travel_m: f64,
    /// Wheel travel in droop before the droop limit engages, m.
    pub droop_travel_m: f64,
    /// Rate of the bump stop and the droop limit at the wheel, N/m.
    pub bump_stop_rate_n_per_m: f64,
    /// Static camber, rad. Negative leans the wheel top toward the car.
    pub static_camber_rad: f64,
    /// Change of camber per metre of bump travel, rad/m.
    pub camber_gain_rad_per_m: f64,
    /// Static toe per wheel, rad. Positive is toe-in.
    pub static_toe_rad: f64,
    /// Roll-centre height above the ground, m. Sets the front-view
    /// inclination of the wheel path, and with it the jacking forces.
    pub roll_center_height_m: f64,
    /// Tangent of the side-view inclination of the wheel path. Positive
    /// values resist pitch: anti-dive at the front; anti-lift under braking
    /// and anti-squat under drive at the rear.
    pub anti_pitch_tan: f64,
}

/// Steering system.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SteeringParams {
    /// Steering-wheel angle per unit road-wheel angle.
    pub ratio: f64,
    /// Steering-wheel angle from centre to full lock, rad.
    pub max_wheel_angle_rad: f64,
    /// Share of ideal Ackermann geometry, 0 (parallel) to 1.
    pub ackermann_fraction: f64,
    /// Caster angle, rad.
    pub caster_rad: f64,
    /// Kingpin inclination, rad.
    pub kingpin_inclination_rad: f64,
    /// Scrub radius at the ground, m. Positive when the contact patch is
    /// outboard of the kingpin axis.
    pub scrub_radius_m: f64,
    /// Mechanical trail at the ground, m.
    pub mechanical_trail_m: f64,
    /// Steer angle lost at the front wheels per unit of total kingpin
    /// moment, rad per N·m. Models the twist of the column, rack mounts
    /// and bushes; it is a main source of understeer in real cars.
    pub compliance_rad_per_nm: f64,
}

/// Brake system.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BrakeParams {
    /// Brake torque per front wheel at full pedal, N·m.
    pub max_torque_front_nm: f64,
    /// Brake torque per rear wheel at full pedal, N·m.
    pub max_torque_rear_nm: f64,
    /// Handbrake torque per rear wheel at full pull, N·m.
    pub handbrake_torque_nm: f64,
    /// Anti-lock system.
    pub abs: AbsParams,
}

/// Anti-lock brake controller: per-wheel pressure modulation on a slip
/// threshold.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AbsParams {
    /// Whether the system is fitted and active.
    pub enabled: bool,
    /// Braking slip ratio magnitude above which pressure is released.
    pub slip_threshold: f64,
    /// Rate at which pressure is released, fraction of full per second.
    pub release_rate_per_s: f64,
    /// Rate at which pressure is reapplied, fraction of full per second.
    pub apply_rate_per_s: f64,
    /// Speed below which the system does not intervene, m/s.
    pub min_speed_m_per_s: f64,
    /// How strongly pressure follows the slip error, fraction of full
    /// pressure per second per unit of slip ratio.
    pub control_gain_per_s: f64,
}

/// Aerodynamics: drag and lift proportional to dynamic pressure.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AeroParams {
    /// Air density, kg/m³.
    pub air_density_kg_per_m3: f64,
    /// Drag area `Cd·A`, m².
    pub drag_area_m2: f64,
    /// Lift area `Cl·A` acting at the front axle, m². Positive is lift.
    pub lift_area_front_m2: f64,
    /// Lift area `Cl·A` acting at the rear axle, m². Positive is lift.
    pub lift_area_rear_m2: f64,
}

/// Which drivetrain module a car uses, with that module's parameters.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DrivetrainParams {
    /// Combustion engine, friction clutch, manual gearbox, one driven axle.
    EngineManual(EngineManualParams),
    /// Two electric motors, one per axle.
    DualMotorElectric(DualMotorParams),
}

/// Which axle a single-axle drivetrain drives.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DrivenAxle {
    /// Front-wheel drive.
    Front,
    /// Rear-wheel drive.
    Rear,
}

/// Differential type.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DifferentialKind {
    /// Open differential: equal torque to both wheels.
    Open,
    /// Clutch-type limited slip: locking torque from preload and input
    /// torque.
    LimitedSlip,
}

/// Parameters of the engine, clutch, gearbox and differential module.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EngineManualParams {
    /// Driven axle.
    pub driven_axle: DrivenAxle,
    /// Engine.
    pub engine: EngineParams,
    /// Clutch.
    pub clutch: ClutchParams,
    /// Gearbox.
    pub gearbox: GearboxParams,
    /// Differential.
    pub differential: DifferentialParams,
}

/// Combustion engine.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EngineParams {
    /// Engine speeds of the full-throttle torque table, rpm, ascending.
    pub torque_curve_rpm: Vec<f64>,
    /// Full-throttle torque at each table speed, N·m.
    pub torque_curve_nm: Vec<f64>,
    /// Rotating inertia of engine and flywheel, kg·m².
    pub inertia_kg_m2: f64,
    /// Idle speed, rpm.
    pub idle_rpm: f64,
    /// Rev-limiter speed, rpm.
    pub redline_rpm: f64,
    /// Constant friction torque, N·m.
    pub friction_torque_nm: f64,
    /// Speed-proportional friction, N·m per rad/s.
    pub friction_nm_per_rad_s: f64,
}

/// Friction clutch.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClutchParams {
    /// Torque capacity when fully engaged, N·m.
    pub max_torque_nm: f64,
    /// Engine speed at which the automatic clutch assist starts to
    /// engage, rpm.
    pub auto_engage_start_rpm: f64,
    /// Engine speed at which the automatic clutch assist is fully
    /// engaged, rpm.
    pub auto_engage_full_rpm: f64,
}

/// Manual gearbox.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GearboxParams {
    /// Forward gear ratios, first gear first.
    pub ratios: Vec<f64>,
    /// Reverse gear ratio, as a positive number.
    pub reverse_ratio: f64,
    /// Final drive ratio.
    pub final_drive: f64,
    /// Time the gearbox spends in neutral during a shift, s.
    pub shift_time_s: f64,
}

/// Differential.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DifferentialParams {
    /// Open or limited slip.
    pub kind: DifferentialKind,
    /// Locking torque present with no input torque, N·m. Ignored when open.
    pub preload_nm: f64,
    /// Locking torque per unit input torque under power. Ignored when open.
    pub lock_fraction_drive: f64,
    /// Locking torque per unit input torque on overrun. Ignored when open.
    pub lock_fraction_coast: f64,
}

/// Parameters of the dual-motor electric module.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DualMotorParams {
    /// Front motor and reduction gear.
    pub front_motor: MotorParams,
    /// Rear motor and reduction gear.
    pub rear_motor: MotorParams,
    /// Share of drive torque sent to the front axle before the torque
    /// vectoring hook adjusts it, 0 to 1.
    pub front_torque_share: f64,
    /// Battery.
    pub battery: BatteryParams,
    /// Regenerative braking.
    pub regen: RegenParams,
    /// Which torque-vectoring strategy to install.
    pub torque_vectoring: TorqueVectoringKind,
}

/// Electric motor with a fixed reduction gear.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MotorParams {
    /// Peak torque at the rotor, N·m.
    pub peak_torque_nm: f64,
    /// Peak mechanical power, W. Torque falls as `power / speed` above the
    /// base speed.
    pub peak_power_w: f64,
    /// Maximum rotor speed, rad/s.
    pub max_speed_rad_per_s: f64,
    /// Reduction ratio from rotor to axle.
    pub reduction_ratio: f64,
    /// Rotor inertia, kg·m².
    pub rotor_inertia_kg_m2: f64,
    /// Combined motor and inverter efficiency, 0 to 1.
    pub efficiency: f64,
    /// Largest regenerative torque at the rotor, N·m.
    pub regen_torque_limit_nm: f64,
    /// Largest regenerative mechanical power, W.
    pub regen_power_limit_w: f64,
}

/// Traction battery, as an energy store with power limits.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BatteryParams {
    /// Usable energy, J.
    pub capacity_j: f64,
    /// State of charge at the start of a session, 0 to 1.
    pub initial_soc: f64,
    /// Largest discharge power, W.
    pub max_discharge_power_w: f64,
    /// Largest charge power, W.
    pub max_charge_power_w: f64,
}

/// Regenerative braking behaviour.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RegenParams {
    /// Regeneration with the accelerator released, as a fraction of each
    /// motor's regeneration limit.
    pub coast_fraction: f64,
    /// Largest share of brake-pedal demand on an axle that regeneration
    /// may take over from the friction brakes, 0 to 1.
    pub brake_share: f64,
    /// Vehicle speed below which regeneration is off, m/s.
    pub fade_low_m_per_s: f64,
    /// Vehicle speed above which regeneration is fully available, m/s.
    pub fade_high_m_per_s: f64,
}

/// Torque-vectoring strategy selection.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TorqueVectoringKind {
    /// Fixed front/rear split, no per-wheel intervention.
    None,
}

impl CarParams {
    /// Distance from the sprung centre of gravity to the rear axle, m.
    pub fn cg_to_rear_axle_m(&self) -> f64 {
        self.body.wheelbase_m - self.body.cg_to_front_axle_m
    }

    /// Total mass including unsprung masses, kg.
    pub fn total_mass_kg(&self) -> f64 {
        self.body.sprung_mass_kg
            + 2.0 * self.suspension.front.unsprung_mass_kg
            + 2.0 * self.suspension.rear.unsprung_mass_kg
    }

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
        fn non_negative(name: &str, v: f64) -> Result<(), String> {
            if v.is_finite() && v >= 0.0 {
                Ok(())
            } else {
                Err(format!("{name} must be zero or positive, got {v}"))
            }
        }
        fn unit(name: &str, v: f64) -> Result<(), String> {
            if v.is_finite() && (0.0..=1.0).contains(&v) {
                Ok(())
            } else {
                Err(format!("{name} must be between 0 and 1, got {v}"))
            }
        }
        let b = &self.body;
        positive("body.sprung_mass_kg", b.sprung_mass_kg)?;
        positive("body.inertia_roll_kg_m2", b.inertia_roll_kg_m2)?;
        positive("body.inertia_pitch_kg_m2", b.inertia_pitch_kg_m2)?;
        positive("body.inertia_yaw_kg_m2", b.inertia_yaw_kg_m2)?;
        positive("body.wheelbase_m", b.wheelbase_m)?;
        positive("body.cg_to_front_axle_m", b.cg_to_front_axle_m)?;
        if b.cg_to_front_axle_m >= b.wheelbase_m {
            return Err("body.cg_to_front_axle_m must be less than the wheelbase".into());
        }
        positive("body.cg_height_m", b.cg_height_m)?;
        positive("body.length_m", b.length_m)?;
        positive("body.width_m", b.width_m)?;
        positive("body.height_m", b.height_m)?;
        positive("body.ground_clearance_m", b.ground_clearance_m)?;
        positive(
            "body.contact_stiffness_n_per_m",
            b.contact_stiffness_n_per_m,
        )?;
        non_negative(
            "body.contact_damping_n_s_per_m",
            b.contact_damping_n_s_per_m,
        )?;
        non_negative("body.contact_friction", b.contact_friction)?;
        for (label, a) in [
            ("front", &self.suspension.front),
            ("rear", &self.suspension.rear),
        ] {
            positive(&format!("suspension.{label}.track_m"), a.track_m)?;
            positive(
                &format!("suspension.{label}.unsprung_mass_kg"),
                a.unsprung_mass_kg,
            )?;
            positive(
                &format!("suspension.{label}.wheel_inertia_kg_m2"),
                a.wheel_inertia_kg_m2,
            )?;
            positive(
                &format!("suspension.{label}.spring_rate_n_per_m"),
                a.spring_rate_n_per_m,
            )?;
            positive(&format!("suspension.{label}.motion_ratio"), a.motion_ratio)?;
            non_negative(
                &format!("suspension.{label}.damper_bump_n_s_per_m"),
                a.damper_bump_n_s_per_m,
            )?;
            non_negative(
                &format!("suspension.{label}.damper_rebound_n_s_per_m"),
                a.damper_rebound_n_s_per_m,
            )?;
            non_negative(
                &format!("suspension.{label}.anti_roll_bar_n_per_m"),
                a.anti_roll_bar_n_per_m,
            )?;
            positive(
                &format!("suspension.{label}.bump_travel_m"),
                a.bump_travel_m,
            )?;
            positive(
                &format!("suspension.{label}.droop_travel_m"),
                a.droop_travel_m,
            )?;
            non_negative(
                &format!("suspension.{label}.bump_stop_rate_n_per_m"),
                a.bump_stop_rate_n_per_m,
            )?;
            non_negative(
                &format!("suspension.{label}.roll_center_height_m"),
                a.roll_center_height_m,
            )?;
        }
        let s = &self.steering;
        positive("steering.ratio", s.ratio)?;
        positive("steering.max_wheel_angle_rad", s.max_wheel_angle_rad)?;
        unit("steering.ackermann_fraction", s.ackermann_fraction)?;
        let br = &self.brakes;
        non_negative("brakes.max_torque_front_nm", br.max_torque_front_nm)?;
        non_negative("brakes.max_torque_rear_nm", br.max_torque_rear_nm)?;
        non_negative("brakes.handbrake_torque_nm", br.handbrake_torque_nm)?;
        positive("brakes.abs.slip_threshold", br.abs.slip_threshold)?;
        positive("brakes.abs.release_rate_per_s", br.abs.release_rate_per_s)?;
        positive("brakes.abs.apply_rate_per_s", br.abs.apply_rate_per_s)?;
        positive("brakes.abs.control_gain_per_s", br.abs.control_gain_per_s)?;
        non_negative("steering.compliance_rad_per_nm", s.compliance_rad_per_nm)?;
        positive(
            "aero.air_density_kg_per_m3",
            self.aero.air_density_kg_per_m3,
        )?;
        non_negative("aero.drag_area_m2", self.aero.drag_area_m2)?;
        match &self.drivetrain {
            DrivetrainParams::EngineManual(d) => {
                let e = &d.engine;
                if e.torque_curve_rpm.len() < 2
                    || e.torque_curve_rpm.len() != e.torque_curve_nm.len()
                {
                    return Err(
                        "drivetrain.engine torque curve needs matching rpm and torque tables of at least two points"
                            .into(),
                    );
                }
                if e.torque_curve_rpm.windows(2).any(|w| w[1] <= w[0]) {
                    return Err("drivetrain.engine.torque_curve_rpm must be ascending".into());
                }
                positive("drivetrain.engine.inertia_kg_m2", e.inertia_kg_m2)?;
                positive("drivetrain.engine.idle_rpm", e.idle_rpm)?;
                if e.redline_rpm <= e.idle_rpm {
                    return Err("drivetrain.engine.redline_rpm must exceed idle_rpm".into());
                }
                non_negative("drivetrain.engine.friction_torque_nm", e.friction_torque_nm)?;
                non_negative(
                    "drivetrain.engine.friction_nm_per_rad_s",
                    e.friction_nm_per_rad_s,
                )?;
                positive("drivetrain.clutch.max_torque_nm", d.clutch.max_torque_nm)?;
                if d.clutch.auto_engage_full_rpm <= d.clutch.auto_engage_start_rpm {
                    return Err(
                        "drivetrain.clutch.auto_engage_full_rpm must exceed auto_engage_start_rpm"
                            .into(),
                    );
                }
                if d.gearbox.ratios.is_empty() || d.gearbox.ratios.len() > 12 {
                    return Err("drivetrain.gearbox.ratios needs 1 to 12 gears".into());
                }
                for (i, r) in d.gearbox.ratios.iter().enumerate() {
                    positive(&format!("drivetrain.gearbox.ratios[{i}]"), *r)?;
                }
                positive("drivetrain.gearbox.reverse_ratio", d.gearbox.reverse_ratio)?;
                positive("drivetrain.gearbox.final_drive", d.gearbox.final_drive)?;
                non_negative("drivetrain.gearbox.shift_time_s", d.gearbox.shift_time_s)?;
                non_negative(
                    "drivetrain.differential.preload_nm",
                    d.differential.preload_nm,
                )?;
                non_negative(
                    "drivetrain.differential.lock_fraction_drive",
                    d.differential.lock_fraction_drive,
                )?;
                non_negative(
                    "drivetrain.differential.lock_fraction_coast",
                    d.differential.lock_fraction_coast,
                )?;
            }
            DrivetrainParams::DualMotorElectric(d) => {
                for (label, m) in [
                    ("front_motor", &d.front_motor),
                    ("rear_motor", &d.rear_motor),
                ] {
                    positive(
                        &format!("drivetrain.{label}.peak_torque_nm"),
                        m.peak_torque_nm,
                    )?;
                    positive(&format!("drivetrain.{label}.peak_power_w"), m.peak_power_w)?;
                    positive(
                        &format!("drivetrain.{label}.max_speed_rad_per_s"),
                        m.max_speed_rad_per_s,
                    )?;
                    positive(
                        &format!("drivetrain.{label}.reduction_ratio"),
                        m.reduction_ratio,
                    )?;
                    positive(
                        &format!("drivetrain.{label}.rotor_inertia_kg_m2"),
                        m.rotor_inertia_kg_m2,
                    )?;
                    if !(m.efficiency > 0.0 && m.efficiency <= 1.0) {
                        return Err(format!(
                            "drivetrain.{label}.efficiency must be in (0, 1], got {}",
                            m.efficiency
                        ));
                    }
                    non_negative(
                        &format!("drivetrain.{label}.regen_torque_limit_nm"),
                        m.regen_torque_limit_nm,
                    )?;
                    non_negative(
                        &format!("drivetrain.{label}.regen_power_limit_w"),
                        m.regen_power_limit_w,
                    )?;
                }
                unit("drivetrain.front_torque_share", d.front_torque_share)?;
                positive("drivetrain.battery.capacity_j", d.battery.capacity_j)?;
                unit("drivetrain.battery.initial_soc", d.battery.initial_soc)?;
                positive(
                    "drivetrain.battery.max_discharge_power_w",
                    d.battery.max_discharge_power_w,
                )?;
                non_negative(
                    "drivetrain.battery.max_charge_power_w",
                    d.battery.max_charge_power_w,
                )?;
                unit("drivetrain.regen.coast_fraction", d.regen.coast_fraction)?;
                unit("drivetrain.regen.brake_share", d.regen.brake_share)?;
                non_negative(
                    "drivetrain.regen.fade_low_m_per_s",
                    d.regen.fade_low_m_per_s,
                )?;
                if d.regen.fade_high_m_per_s <= d.regen.fade_low_m_per_s {
                    return Err(
                        "drivetrain.regen.fade_high_m_per_s must exceed fade_low_m_per_s".into(),
                    );
                }
            }
        }
        Ok(())
    }
}
