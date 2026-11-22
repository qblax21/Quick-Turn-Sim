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
    /// Traction control. Absent from a data file means not fitted.
    #[serde(default)]
    pub traction_control: TractionControlParams,
}

/// Traction control: the accelerator demand is scaled back while the
/// fastest-spinning wheel exceeds a drive slip threshold.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TractionControlParams {
    /// Whether the system is fitted and active.
    pub enabled: bool,
    /// Drive slip ratio above which torque is cut.
    pub slip_threshold: f64,
    /// Fastest rate at which torque is cut, fraction of full per second.
    pub release_rate_per_s: f64,
    /// Fastest rate at which torque is restored, fraction of full per second.
    pub apply_rate_per_s: f64,
    /// How strongly the torque fraction follows the slip error, fraction
    /// of full per second per unit of slip ratio.
    pub control_gain_per_s: f64,
    /// Speed below which slip is measured against this speed instead of
    /// the car's, m/s, so a launch from rest is not read as infinite slip.
    pub min_speed_m_per_s: f64,
}

impl Default for TractionControlParams {
    fn default() -> Self {
        Self {
            enabled: false,
            slip_threshold: 0.1,
            release_rate_per_s: 10.0,
            apply_rate_per_s: 5.0,
            control_gain_per_s: 60.0,
            min_speed_m_per_s: 2.0,
        }
    }
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
    /// Speeds of the power-assist table, km/h, ascending.
    pub assist_speed_kmh: Vec<f64>,
    /// Power-assist boost gain at each table speed: assist torque per N·m
    /// of driver torque above the threshold. Zero is an unassisted rack.
    /// Linear between table speeds, held beyond its ends.
    pub assist_gain: Vec<f64>,
    /// Driver torque at the column below which the assist gives nothing,
    /// N·m: the on-centre zone where the driver feels the rack unassisted.
    pub assist_threshold_nm: f64,
    /// Coulomb friction of the rack, column and joints, referred to the
    /// column, N·m. Felt against the direction the wheel is turned.
    pub rack_friction_nm: f64,
    /// Rotating inertia of the road wheels about their kingpins and of the
    /// rack, referred to the column, kg·m². The steering wheel itself is
    /// not included: the driver holds a real one.
    pub steering_inertia_kg_m2: f64,
    /// Stiffness of the torsion bar in the column that the power steering
    /// senses the driver's torque with, N·m per rad. What the driver feels
    /// is this bar's twist (D-088).
    pub torsion_bar_nm_per_rad: f64,
    /// Inertia on the far side of the torsion bar, referred to the column,
    /// kg·m²: the assist motor through its gearing, the rack and the road
    /// wheels about their kingpins.
    pub assist_side_inertia_kg_m2: f64,
    /// Damping of the twist, as a share of critical: the assist motor's
    /// own damping and the rack's.
    pub torsion_damping_ratio: f64,
}

impl SteeringParams {
    /// Power-assist boost gain at a speed, m/s.
    pub fn assist_gain_at(&self, speed_m_s: f64) -> f64 {
        let kmh = speed_m_s.abs() * 3.6;
        let (xs, ys) = (&self.assist_speed_kmh, &self.assist_gain);
        let (Some(&first), Some(&last)) = (ys.first(), ys.last()) else {
            return 0.0;
        };
        if kmh <= xs[0] {
            return first.max(0.0);
        }
        for i in 1..xs.len() {
            if kmh <= xs[i] {
                let t = (kmh - xs[i - 1]) / (xs[i] - xs[i - 1]);
                return (ys[i - 1] + t * (ys[i] - ys[i - 1])).max(0.0);
            }
        }
        last.max(0.0)
    }

    /// Torque the driver holds at the column against a steering load
    /// `load_nm` (the rack load referred to the column), with the assist
    /// at `speed_m_s`. A boost-curve assist: below the threshold the
    /// driver carries everything; above it the assist adds its gain times
    /// the driver's torque beyond the threshold, so the driver carries
    /// `(|load| + K·T0) / (1 + K)`. Same sign as the load.
    pub fn driver_torque(&self, load_nm: f64, speed_m_s: f64) -> f64 {
        let t0 = self.assist_threshold_nm;
        let magnitude = load_nm.abs();
        if magnitude <= t0 {
            return load_nm;
        }
        let k = self.assist_gain_at(speed_m_s);
        ((magnitude + k * t0) / (1.0 + k)).copysign(load_nm)
    }
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
    /// Front disc temperature model, per wheel.
    pub thermal_front: BrakeThermalParams,
    /// Rear disc temperature model, per wheel.
    pub thermal_rear: BrakeThermalParams,
    /// Disc temperatures of the friction table, °C, ascending.
    pub fade_temp_c: Vec<f64>,
    /// Pad friction at each table temperature as a share of its nominal
    /// value: the brake torque scales with it. Linear between points, held
    /// beyond the ends.
    pub fade_friction_factor: Vec<f64>,
}

/// One brake disc as a single thermal mass: heated by the brake's friction
/// work, cooled by convection that grows with speed (more with a duct) and
/// by radiation.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BrakeThermalParams {
    /// Heat capacity of the disc and the parts that heat with it, J/K.
    pub heat_capacity_j_per_k: f64,
    /// Share of the brake's friction work that goes into the disc; the
    /// rest heats the pads and is not followed.
    pub disc_heat_share: f64,
    /// Convective cooling at rest, W/K.
    pub convection_w_per_k: f64,
    /// Extra convective cooling per m/s of road speed, W/K per m/s.
    pub convection_speed_gain: f64,
    /// Emissivity times radiating area, m².
    pub radiation_area_m2: f64,
    /// Extra speed-dependent cooling with the duct fully open, as a share
    /// of `convection_speed_gain`.
    pub duct_cooling_gain: f64,
    /// Duct opening as delivered, 0 (none) to 1 (fully open).
    pub duct_opening: f64,
}

/// Stefan-Boltzmann constant, W/(m²·K⁴).
const STEFAN_BOLTZMANN: f64 = 5.670_374_419e-8;

impl BrakeThermalParams {
    /// Heat leaving the disc at `temp_c` in air at `air_c`, at road speed
    /// `speed_m_s`, W.
    pub fn cooling_w(&self, temp_c: f64, air_c: f64, speed_m_s: f64) -> f64 {
        let speed = speed_m_s.abs();
        let h = self.convection_w_per_k
            + self.convection_speed_gain
                * speed
                * (1.0 + self.duct_cooling_gain * self.duct_opening.clamp(0.0, 1.0));
        let (t, a) = (temp_c + 273.15, air_c + 273.15);
        h * (temp_c - air_c)
            + STEFAN_BOLTZMANN * self.radiation_area_m2 * (t * t * t * t - a * a * a * a)
    }
}

impl BrakeParams {
    /// Pad friction factor at a disc temperature, °C.
    pub fn friction_factor(&self, temp_c: f64) -> f64 {
        let (xs, ys) = (&self.fade_temp_c, &self.fade_friction_factor);
        let (Some(&first), Some(&last)) = (ys.first(), ys.last()) else {
            return 1.0;
        };
        if temp_c <= xs[0] {
            return first;
        }
        for i in 1..xs.len() {
            if temp_c <= xs[i] {
                let t = (temp_c - xs[i - 1]) / (xs[i] - xs[i - 1]);
                return ys[i - 1] + t * (ys[i] - ys[i - 1]);
            }
        }
        last
    }
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
    /// Clutch-pack (Salisbury) limited slip: locking torque from a spring
    /// preload and from ramps that turn input torque into clutch clamping.
    #[serde(alias = "limited_slip")]
    ClutchPack,
    /// Locked (a spool): both wheels turn together.
    Locked,
}

impl DifferentialKind {
    /// The garage's number for this mode: 0 open, 1 clutch pack, 2 locked.
    pub fn code(self) -> f64 {
        match self {
            DifferentialKind::Open => 0.0,
            DifferentialKind::ClutchPack => 1.0,
            DifferentialKind::Locked => 2.0,
        }
    }

    /// The mode for a garage number.
    pub fn from_code(code: f64) -> Option<Self> {
        match code.round() as i64 {
            0 => Some(DifferentialKind::Open),
            1 => Some(DifferentialKind::ClutchPack),
            2 => Some(DifferentialKind::Locked),
            _ => None,
        }
    }
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
    /// Rev-limiter speed, rpm. The limiter cuts fuel here, so the engine
    /// cannot drive itself past it; the wheels can, through the clutch.
    pub redline_rpm: f64,
    /// Constant friction torque, N·m.
    pub friction_torque_nm: f64,
    /// Speed-proportional friction, N·m per rad/s.
    pub friction_nm_per_rad_s: f64,
    /// Speed above which an over-rev starts to harm the engine (valves
    /// touching pistons, bearing loads), rpm. Only counts when engine
    /// damage is switched on.
    pub overrev_damage_rpm: f64,
    /// Speed at which an over-rev destroys the engine at once, rpm.
    pub overrev_failure_rpm: f64,
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
    /// Pedal travel from the top, 0 to 1, over which nothing happens: the
    /// play before the release bearing meets the diaphragm spring.
    pub free_travel: f64,
    /// Pedal travel at the middle of the bite zone, 0 to 1.
    pub bite_point: f64,
    /// Width of the bite zone in pedal travel, 0 to 1. Across it the
    /// clutch goes from free to [`BITE_TOP_ENGAGEMENT`] of its capacity;
    /// beyond its lower edge, down to the floor, the clutch is free.
    pub bite_width: f64,
}

/// Share of full clutch capacity at the released end of the bite zone.
/// Above the zone the capacity rises on to full at the end of the free
/// travel; at 1.5 times engine torque it already holds the engine at the
/// top of the zone, so the zone is where the car is driven on the clutch.
pub const BITE_TOP_ENGAGEMENT: f64 = 0.5;

impl ClutchParams {
    /// Pedal travel beyond which the clutch transmits nothing, 0 to 1.
    pub fn release_travel(&self) -> f64 {
        self.bite_point + 0.5 * self.bite_width
    }

    /// Clutch engagement, 0 (free) to 1 (fully clamped), for a pedal
    /// travel from 0 (released) to 1 (floored).
    ///
    /// Full engagement through the free travel; then a linear fall to
    /// [`BITE_TOP_ENGAGEMENT`] at the top of the bite zone; then a
    /// quadratic fall across the zone that reaches zero with zero slope,
    /// so the first touch of the plates is gentle; then free to the floor.
    pub fn engagement(&self, travel: f64) -> f64 {
        let t = travel.clamp(0.0, 1.0);
        let bite_top = self.bite_point - 0.5 * self.bite_width;
        let release = self.release_travel();
        if t <= self.free_travel {
            1.0
        } else if t < bite_top {
            let s = (t - self.free_travel) / (bite_top - self.free_travel);
            1.0 - (1.0 - BITE_TOP_ENGAGEMENT) * s
        } else if t < release {
            let s = (release - t) / self.bite_width;
            BITE_TOP_ENGAGEMENT * s * s
        } else {
            0.0
        }
    }
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
    /// Open, clutch pack or locked.
    pub kind: DifferentialKind,
    /// Clutch-pack spring preload: locking torque with no input torque,
    /// N·m.
    pub preload_nm: f64,
    /// Ramp angle on the drive side, degrees, as limited-slip makers quote
    /// it: lower locks harder; 90 gives no locking from torque.
    pub power_ramp_deg: f64,
    /// Ramp angle on the overrun side, degrees.
    pub coast_ramp_deg: f64,
    /// Friction faces in the clutch packs, both sides together.
    pub friction_faces: f64,
    /// Friction coefficient of the oil-wetted plates.
    pub plate_friction: f64,
    /// Mean clutch radius over the radius at which the pinion shaft bears
    /// on the ramps.
    pub clutch_to_ramp_radius: f64,
}

impl DifferentialParams {
    /// Locking torque per N·m of input torque for a ramp angle: the ramps
    /// turn the input torque into a clamping force
    /// `F = T / (r_p · tan θ)`, which the packs turn into
    /// `μ · F · r_c · n / 2` of locking torque between the two sides.
    pub fn lock_per_input(&self, ramp_deg: f64) -> f64 {
        if ramp_deg >= 89.9 {
            return 0.0;
        }
        let tan = qts_core::math::fm::tan(ramp_deg.clamp(1.0, 89.9).to_radians());
        self.plate_friction * self.friction_faces * self.clutch_to_ramp_radius / (2.0 * tan)
    }

    /// Locking torque, N·m, for an input torque at the differential case
    /// (positive driving, negative on overrun). Infinite when locked, zero
    /// when open.
    pub fn locking_torque(&self, input_nm: f64) -> f64 {
        match self.kind {
            DifferentialKind::Open => 0.0,
            DifferentialKind::Locked => f64::INFINITY,
            DifferentialKind::ClutchPack => {
                let ramp = if input_nm >= 0.0 {
                    self.power_ramp_deg
                } else {
                    self.coast_ramp_deg
                };
                self.preload_nm + self.lock_per_input(ramp) * input_nm.abs()
            }
        }
    }

    /// The ramp angle that gives a locking fraction, degrees (for setups
    /// written as lock percentages before D-084).
    pub fn ramp_for_lock(&self, fraction: f64) -> f64 {
        if fraction <= 0.0 {
            return 90.0;
        }
        let k = self.plate_friction * self.friction_faces * self.clutch_to_ramp_radius / 2.0;
        qts_core::math::fm::atan(k / fraction)
            .to_degrees()
            .clamp(1.0, 90.0)
    }
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
        for (axle, th) in [("front", &br.thermal_front), ("rear", &br.thermal_rear)] {
            positive(
                &format!("brakes.thermal_{axle}.heat_capacity_j_per_k"),
                th.heat_capacity_j_per_k,
            )?;
            unit(
                &format!("brakes.thermal_{axle}.disc_heat_share"),
                th.disc_heat_share,
            )?;
            non_negative(
                &format!("brakes.thermal_{axle}.convection_w_per_k"),
                th.convection_w_per_k,
            )?;
            non_negative(
                &format!("brakes.thermal_{axle}.convection_speed_gain"),
                th.convection_speed_gain,
            )?;
            non_negative(
                &format!("brakes.thermal_{axle}.radiation_area_m2"),
                th.radiation_area_m2,
            )?;
            non_negative(
                &format!("brakes.thermal_{axle}.duct_cooling_gain"),
                th.duct_cooling_gain,
            )?;
            unit(
                &format!("brakes.thermal_{axle}.duct_opening"),
                th.duct_opening,
            )?;
        }
        if br.fade_temp_c.is_empty()
            || br.fade_temp_c.len() != br.fade_friction_factor.len()
            || br.fade_temp_c.windows(2).any(|w| w[1] <= w[0])
            || br
                .fade_friction_factor
                .iter()
                .any(|f| !f.is_finite() || *f <= 0.0 || *f > 1.5)
        {
            return Err(
                "brakes.fade_temp_c and fade_friction_factor need matching tables, temperatures ascending, factors above 0 and at most 1.5"
                    .into(),
            );
        }
        positive("brakes.abs.slip_threshold", br.abs.slip_threshold)?;
        positive("brakes.abs.release_rate_per_s", br.abs.release_rate_per_s)?;
        positive("brakes.abs.apply_rate_per_s", br.abs.apply_rate_per_s)?;
        positive("brakes.abs.control_gain_per_s", br.abs.control_gain_per_s)?;
        let tc = &self.traction_control;
        positive("traction_control.slip_threshold", tc.slip_threshold)?;
        positive("traction_control.release_rate_per_s", tc.release_rate_per_s)?;
        positive("traction_control.apply_rate_per_s", tc.apply_rate_per_s)?;
        positive("traction_control.control_gain_per_s", tc.control_gain_per_s)?;
        positive("traction_control.min_speed_m_per_s", tc.min_speed_m_per_s)?;
        non_negative("steering.compliance_rad_per_nm", s.compliance_rad_per_nm)?;
        if s.assist_speed_kmh.is_empty()
            || s.assist_speed_kmh.len() != s.assist_gain.len()
            || s.assist_speed_kmh.windows(2).any(|w| w[1] <= w[0])
            || s.assist_gain
                .iter()
                .any(|g| !g.is_finite() || *g < 0.0 || *g > 20.0)
        {
            return Err(
                "steering.assist_speed_kmh and assist_gain need matching tables, speeds ascending, gains 0 to 20"
                    .into(),
            );
        }
        non_negative("steering.assist_threshold_nm", s.assist_threshold_nm)?;
        non_negative("steering.rack_friction_nm", s.rack_friction_nm)?;
        non_negative("steering.steering_inertia_kg_m2", s.steering_inertia_kg_m2)?;
        positive("steering.torsion_bar_nm_per_rad", s.torsion_bar_nm_per_rad)?;
        positive(
            "steering.assist_side_inertia_kg_m2",
            s.assist_side_inertia_kg_m2,
        )?;
        positive("steering.torsion_damping_ratio", s.torsion_damping_ratio)?;
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
                if !(e.overrev_damage_rpm > e.redline_rpm
                    && e.overrev_failure_rpm > e.overrev_damage_rpm)
                {
                    return Err(
                        "drivetrain.engine needs redline_rpm < overrev_damage_rpm < overrev_failure_rpm"
                            .into(),
                    );
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
                let cl = &d.clutch;
                let ok = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
                if !(ok(cl.free_travel) && ok(cl.bite_point) && ok(cl.bite_width))
                    || cl.bite_width < 0.02
                    || cl.bite_point - 0.5 * cl.bite_width <= cl.free_travel
                    || cl.release_travel() > 0.95
                {
                    return Err(
                        "drivetrain.clutch needs free_travel < bite zone, a bite zone at least 0.02 wide, released by 0.95 of travel"
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
                let df = &d.differential;
                for (name, ramp) in [
                    ("power_ramp_deg", df.power_ramp_deg),
                    ("coast_ramp_deg", df.coast_ramp_deg),
                ] {
                    if !(ramp.is_finite() && (10.0..=90.0).contains(&ramp)) {
                        return Err(format!(
                            "drivetrain.differential.{name} must be between 10 and 90 degrees, got {ramp}"
                        ));
                    }
                }
                non_negative("drivetrain.differential.friction_faces", df.friction_faces)?;
                unit("drivetrain.differential.plate_friction", df.plate_friction)?;
                positive(
                    "drivetrain.differential.clutch_to_ramp_radius",
                    df.clutch_to_ramp_radius,
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
