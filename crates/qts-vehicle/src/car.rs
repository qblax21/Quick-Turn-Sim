// SPDX-License-Identifier: Apache-2.0
//! The car: a sprung rigid body, four unsprung masses, tires, brakes,
//! steering, aerodynamics and a drivetrain module.
//!
//! # Equations of motion
//!
//! The sprung body has six degrees of freedom. Each unsprung mass is a
//! point mass at the wheel centre that moves with the body except along its
//! travel direction, where it has its own travel `ζ` (positive in bump).
//! The travel direction is the body z axis tilted by the suspension
//! geometry: sideways by the roll-centre height and fore-aft by the
//! anti-pitch angle. Jacking, anti-dive and anti-squat follow from that
//! geometry instead of being added as separate forces, which keeps the
//! model energy-consistent. With
//! `a` and `α` the body's linear and angular acceleration in body
//! coordinates, the ten unknowns `(a, α, ζ̈₁…ζ̈₄)` satisfy a symmetric
//! positive-definite linear system that is assembled and solved every tick:
//!
//! - total linear momentum of body plus wheels,
//! - total angular momentum about the body centre of gravity,
//! - for each wheel, Newton's law along its travel direction, where the
//!   suspension force acts.
//!
//! Tire load is therefore a result of the dynamics (tire spring on unsprung
//! mass), not a weight-transfer formula. Integration is semi-implicit
//! Euler. Wheel gyroscopic effects are neglected, and tire forces act on
//! the body at the contact patch.

use crate::drivetrain::{self, DriveContext, DriveControls, DriveTelemetry, Drivetrain};
use crate::ground::Ground;
use crate::params::{AxleParams, CarParams};
use qts_core::math::{clamp, fm, solve_spd, Quat, Vec3};
use qts_core::snapshot::{Snapshot, SnapshotError, StateReader, StateWriter};
use qts_tire::model::{self as tire_model, TireInput, TireState};
use qts_tire::{vertical_load, vertical_stiffness, TireParams};

/// Index of the front-left wheel.
pub const FL: usize = 0;
/// Index of the front-right wheel.
pub const FR: usize = 1;
/// Index of the rear-left wheel.
pub const RL: usize = 2;
/// Index of the rear-right wheel.
pub const RR: usize = 3;

/// Tangential speed over which body-on-ground friction saturates, m/s.
const CONTACT_FRICTION_SPEED: f64 = 0.5;
/// Lower bound on the rolling radius, m, so a crushed tire cannot produce a
/// zero or negative radius.
const MIN_ROLLING_RADIUS: f64 = 0.05;
/// Smallest brake pedal that counts as braking for the anti-lock logic.
const ABS_PEDAL_THRESHOLD: f64 = 0.01;
/// Lowest pressure fraction the anti-lock system will release to.
const ABS_MIN_FACTOR: f64 = 0.02;
/// Accelerator travel below which traction control stands down.
const TC_PEDAL_THRESHOLD: f64 = 0.01;
/// Lowest torque fraction traction control may command.
const TC_MIN_FACTOR: f64 = 0.02;
/// Accelerator demand traction control never cuts below.
const TC_PEDAL_FLOOR: f64 = 0.12;
/// Time constant of the steering compliance response, s.
const STEER_COMPLIANCE_LAG_S: f64 = 0.02;
/// Cutoff of the filters on steering-column rate and acceleration, Hz.
/// The column angle comes from the driver's input at the physics rate,
/// quantised; the filters keep its derivatives usable.
const COLUMN_FILTER_HZ: f64 = 20.0;
/// Column rate at which rack friction reaches about three quarters of its
/// full value, rad/s: a smooth stand-in for the stick-slip step.
const RACK_FRICTION_SMOOTHING_RAD_S: f64 = 0.15;

/// What the driver is asking for this tick, already decoded from the input
/// frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriverControls {
    /// Steering, -1 (full right) to 1 (full left).
    pub steer: f64,
    /// Accelerator, 0 to 1.
    pub throttle: f64,
    /// Brake pedal, 0 to 1.
    pub brake: f64,
    /// Clutch pedal, 0 (engaged) to 1 (pressed).
    pub clutch: f64,
    /// Handbrake, 0 to 1.
    pub handbrake: f64,
    /// Sequential upshift requested this tick.
    pub shift_up: bool,
    /// Sequential downshift requested this tick.
    pub shift_down: bool,
    /// Direct gear selection.
    pub gear_request: Option<i8>,
    /// Starter motor held.
    pub starter: bool,
}

/// Driver assists, fixed for a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assists {
    /// The clutch engages and releases by itself.
    pub auto_clutch: bool,
    /// Gears are selected automatically.
    pub auto_shift: bool,
    /// Traction control is switched on, on a car that has it.
    pub traction_control: bool,
    /// Over-revving the engine harms it, down to failure. Off, an over-rev
    /// is reported but costs nothing.
    pub engine_damage: bool,
}

impl Default for Assists {
    fn default() -> Self {
        Self {
            auto_clutch: false,
            auto_shift: false,
            traction_control: true,
            engine_damage: false,
        }
    }
}

/// Surroundings of the car, fixed for a session.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Environment {
    /// Gravitational acceleration, m/s².
    pub gravity: f64,
    /// Air temperature, °C.
    pub air_temp_c: f64,
    /// Road surface temperature, °C.
    pub road_temp_c: f64,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            gravity: qts_core::constants::GRAVITY_M_S2,
            air_temp_c: 20.0,
            road_temp_c: 25.0,
        }
    }
}

/// Complete dynamic state of the car, apart from the drivetrain module's
/// own state.
#[derive(Clone, Debug, PartialEq)]
pub struct CarState {
    /// Position of the sprung centre of gravity, world frame, m.
    pub pos: Vec3,
    /// Orientation, body to world.
    pub rot: Quat,
    /// Velocity of the sprung centre of gravity, world frame, m/s.
    pub vel: Vec3,
    /// Angular velocity, body frame, rad/s.
    pub ang_vel: Vec3,
    /// Suspension travel per wheel, m, positive in bump.
    pub susp: [f64; 4],
    /// Suspension travel rate per wheel, m/s.
    pub susp_rate: [f64; 4],
    /// Wheel spin rate, rad/s.
    pub wheel_omega: [f64; 4],
    /// Wheel rotation angle, rad, wrapped to ±π. Used for drawing.
    pub wheel_angle: [f64; 4],
    /// Tire states.
    pub tires: [TireState; 4],
    /// Anti-lock pressure fraction per wheel, 0 to 1.
    pub abs_factor: [f64; 4],
    /// Traction-control torque fraction, 0 to 1.
    pub tc_factor: f64,
    /// Steer angle lost at the front wheels to steering-system compliance,
    /// rad. Negative values reduce a left steer.
    pub steer_compliance: f64,
    /// Brake disc temperature per wheel, °C.
    pub brake_temp_c: [f64; 4],
    /// Steering-wheel angle of the last tick, rad.
    pub column_angle: f64,
    /// Filtered steering-wheel rate, rad/s.
    pub column_rate: f64,
    /// Filtered steering-wheel acceleration, rad/s².
    pub column_accel: f64,
    /// False until the column angle has been seen once since the car was
    /// placed, so a turned wheel at the start is not read as a fast turn.
    pub column_primed: bool,
}

/// The parts of the steering torque at the column, N·m, positive turning
/// the steering wheel to the left. `total` is what reaches the driver.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SteeringOutput {
    /// Rack load from the tires and geometry, referred to the column,
    /// before the assist.
    pub load_nm: f64,
    /// Part of `load_nm` from the vertical loads through caster and
    /// kingpin inclination.
    pub weight_nm: f64,
    /// Part of `load_nm` from the gyroscopic moment of the spinning front
    /// wheels.
    pub gyro_nm: f64,
    /// What the power assist carries (opposite sign to the load).
    pub assist_nm: f64,
    /// Rack and column friction.
    pub friction_nm: f64,
    /// Rack and road-wheel inertia.
    pub inertia_nm: f64,
    /// Assist gain in use.
    pub assist_gain: f64,
}

/// Per-wheel values from the last tick, for telemetry and display.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelOutput {
    /// Vertical tire load, N.
    pub load: f64,
    /// Longitudinal tire force in the wheel frame, N.
    pub fx: f64,
    /// Lateral tire force in the wheel frame, N.
    pub fy: f64,
    /// Aligning moment, N·m.
    pub mz: f64,
    /// Transient slip ratio.
    pub slip_ratio: f64,
    /// Transient slip angle, rad.
    pub slip_angle: f64,
    /// Camber relative to the road, toward the wheel's +y side, rad.
    pub camber: f64,
    /// Steer angle of the wheel relative to the body, rad.
    pub steer_angle: f64,
    /// Suspension travel, m.
    pub susp_travel: f64,
    /// Suspension force on the body, N.
    pub susp_force: f64,
    /// Spin rate, rad/s.
    pub omega: f64,
    /// Rolling radius, m.
    pub rolling_radius: f64,
    /// Index of the surface under the wheel.
    pub surface_index: u16,
    /// Friction-brake torque applied, N·m.
    pub brake_torque: f64,
    /// Driveline torque received, N·m.
    pub drive_torque: f64,
    /// Tread surface temperature, °C.
    pub temp_surface_c: f64,
    /// Tread core temperature, °C.
    pub temp_core_c: f64,
    /// Carcass temperature, °C.
    pub temp_carcass_c: f64,
    /// Inflation pressure, Pa absolute.
    pub pressure_pa: f64,
    /// Brake disc temperature, °C.
    pub brake_temp_c: f64,
    /// Pad friction factor from the disc temperature (1 nominal, lower
    /// when faded).
    pub brake_friction_factor: f64,
    /// Heat going into the disc from braking, W.
    pub brake_heat_w: f64,
    /// Heat leaving the disc, W.
    pub brake_cooling_w: f64,
    /// Share of the usable tread worn away, 0 to 1.
    pub tire_wear: f64,
    /// Wheel centre position in the body frame, m.
    pub center_body: [f64; 3],
}

/// Whole-car values from the last tick, for telemetry, display and force
/// feedback. Not part of the state: it is recomputed every tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarOutput {
    /// Per-wheel values.
    pub wheels: [WheelOutput; 4],
    /// Raw steering torque at the column, N·m. Positive when the tires are
    /// trying to turn the steering wheel to the left.
    pub steering_torque_nm: f64,
    /// Steering-wheel angle, rad, positive left.
    pub steering_wheel_angle: f64,
    /// The parts of `steering_torque_nm`.
    pub steering: SteeringOutput,
    /// Acceleration of the sprung centre of gravity in body axes, m/s².
    pub accel_body: [f64; 3],
    /// Velocity in body axes, m/s.
    pub vel_body: [f64; 3],
    /// Speed over the ground, m/s.
    pub speed: f64,
    /// Yaw rate, rad/s.
    pub yaw_rate: f64,
    /// Drivetrain values.
    pub drive: DriveTelemetry,
    /// Aerodynamic drag force, N.
    pub aero_drag_n: f64,
    /// Number of body contact points touching the ground.
    pub body_contacts: u8,
}

#[derive(Clone, Copy, Debug)]
struct Corner {
    /// Wheel centre in the body frame at zero travel.
    mount: Vec3,
    /// +1 on the left, -1 on the right.
    side: f64,
    /// 0 front, 1 rear.
    axle: usize,
    unsprung_mass: f64,
    wheel_inertia: f64,
    wheel_rate: f64,
    damp_bump: f64,
    damp_rebound: f64,
    static_force: f64,
    /// Direction the wheel centre moves per unit of vertical travel, body
    /// frame. Its z component is 1; the x and y components come from the
    /// suspension geometry (anti-pitch and roll-centre height).
    travel_dir: Vec3,
}

/// A car instance: parameters, state and drivetrain module.
pub struct Car {
    params: CarParams,
    tires: [TireParams; 2],
    corners: [Corner; 4],
    contact_points: [Vec3; 8],
    inertia: Vec3,
    /// Dynamic state.
    pub state: CarState,
    drivetrain: Box<dyn Drivetrain>,
    output: CarOutput,
    initial_tire_temp_c: f64,
    legacy_steering: Option<f64>,
}

impl std::fmt::Debug for Car {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Car")
            .field("id", &self.params.id)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

fn axle_of(params: &CarParams, axle: usize) -> &AxleParams {
    if axle == 0 {
        &params.suspension.front
    } else {
        &params.suspension.rear
    }
}

impl Car {
    /// Builds a car at rest at the world origin on ground of height zero.
    ///
    /// `gravity` fixes the static spring preload, so the car sits at its
    /// design ride height. Tires start at `tire_temp_c`.
    pub fn new(
        params: CarParams,
        tire_front: TireParams,
        tire_rear: TireParams,
        gravity: f64,
        tire_temp_c: f64,
    ) -> Result<Self, String> {
        params.validate()?;
        tire_front.validate()?;
        tire_rear.validate()?;
        let tires = [tire_front, tire_rear];
        let b = &params.body;
        let a = b.cg_to_front_axle_m;
        let c = params.cg_to_rear_axle_m();
        let l = b.wheelbase_m;
        let mut corners = [Corner {
            mount: Vec3::ZERO,
            side: 1.0,
            axle: 0,
            unsprung_mass: 0.0,
            wheel_inertia: 0.0,
            wheel_rate: 0.0,
            damp_bump: 0.0,
            damp_rebound: 0.0,
            static_force: 0.0,
            travel_dir: Vec3::Z,
        }; 4];
        for (i, corner) in corners.iter_mut().enumerate() {
            let axle = i / 2;
            let side = if i % 2 == 0 { 1.0 } else { -1.0 };
            let ax = axle_of(&params, axle);
            let share = if axle == 0 { c / l } else { a / l };
            let static_force = 0.5 * b.sprung_mass_kg * gravity * share;
            let tire = &tires[axle];
            let static_load = static_force + ax.unsprung_mass_kg * gravity;
            let static_radius =
                tire.geometry.unloaded_radius_m - static_load / tire.vertical.stiffness_n_per_m;
            if static_radius <= MIN_ROLLING_RADIUS {
                return Err(format!(
                    "tire {} is crushed flat by the static load of {}",
                    tire.id, params.id
                ));
            }
            let mr2 = ax.motion_ratio * ax.motion_ratio;
            *corner = Corner {
                mount: Vec3::new(
                    if axle == 0 { a } else { -c },
                    side * 0.5 * ax.track_m,
                    -(b.cg_height_m - static_radius),
                ),
                side,
                axle,
                unsprung_mass: ax.unsprung_mass_kg,
                wheel_inertia: ax.wheel_inertia_kg_m2,
                wheel_rate: ax.spring_rate_n_per_m * mr2,
                damp_bump: ax.damper_bump_n_s_per_m * mr2,
                damp_rebound: ax.damper_rebound_n_s_per_m * mr2,
                static_force,
                travel_dir: Vec3::new(
                    if axle == 0 {
                        ax.anti_pitch_tan
                    } else {
                        -ax.anti_pitch_tan
                    },
                    ax.roll_center_height_m / (side * 0.5 * ax.track_m),
                    1.0,
                ),
            };
        }
        // Body box for ground contact, centred between the axles.
        let x_mid = 0.5 * (a - c);
        let z_bottom = -(b.cg_height_m - b.ground_clearance_m);
        let z_top = z_bottom + b.height_m;
        let mut contact_points = [Vec3::ZERO; 8];
        let mut n = 0;
        for sx in [1.0, -1.0] {
            for sy in [1.0, -1.0] {
                for z in [z_bottom, z_top] {
                    contact_points[n] =
                        Vec3::new(x_mid + sx * 0.5 * b.length_m, sy * 0.5 * b.width_m, z);
                    n += 1;
                }
            }
        }
        let inertia = Vec3::new(
            b.inertia_roll_kg_m2,
            b.inertia_pitch_kg_m2,
            b.inertia_yaw_kg_m2,
        );
        let drivetrain = drivetrain::build(&params.drivetrain);
        let state = CarState {
            pos: Vec3::new(0.0, 0.0, b.cg_height_m),
            rot: Quat::IDENTITY,
            vel: Vec3::ZERO,
            ang_vel: Vec3::ZERO,
            susp: [0.0; 4],
            susp_rate: [0.0; 4],
            wheel_omega: [0.0; 4],
            wheel_angle: [0.0; 4],
            tires: [TireState::new(tire_temp_c); 4],
            abs_factor: [1.0; 4],
            tc_factor: 1.0,
            steer_compliance: 0.0,
            brake_temp_c: [tire_temp_c; 4],
            column_angle: 0.0,
            column_rate: 0.0,
            column_accel: 0.0,
            column_primed: false,
        };
        let mut car = Self {
            params,
            tires,
            corners,
            contact_points,
            inertia,
            state,
            drivetrain,
            output: CarOutput::default(),
            initial_tire_temp_c: tire_temp_c,
            legacy_steering: None,
        };
        car.refresh_static_output();
        Ok(car)
    }

    /// Parameters of this car.
    pub fn params(&self) -> &CarParams {
        &self.params
    }

    /// Tire parameters for an axle (0 front, 1 rear).
    pub fn tire_params(&self, axle: usize) -> &TireParams {
        &self.tires[axle.min(1)]
    }

    /// Values computed during the last tick.
    pub fn output(&self) -> &CarOutput {
        &self.output
    }

    /// The drivetrain module.
    pub fn drivetrain(&self) -> &dyn Drivetrain {
        self.drivetrain.as_ref()
    }

    /// Switches to the steering-torque model from before D-075 (a fixed
    /// assist fraction, no friction, inertia or gyroscopic moment), with
    /// the given assist fraction, or back to the current model with
    /// `None`. For before-and-after comparisons only: not part of the
    /// state and never set in a session.
    #[doc(hidden)]
    pub fn set_legacy_steering(&mut self, assist_fraction: Option<f64>) {
        self.legacy_steering = assist_fraction;
    }

    /// Puts the car at rest at `(x, y)` with the given heading on ground of
    /// height `ground_height`, with every subsystem back in its initial
    /// state.
    pub fn place(&mut self, x: f64, y: f64, heading: f64, ground_height: f64) {
        self.state = CarState {
            pos: Vec3::new(x, y, ground_height + self.params.body.cg_height_m),
            rot: Quat::from_yaw(heading),
            vel: Vec3::ZERO,
            ang_vel: Vec3::ZERO,
            susp: [0.0; 4],
            susp_rate: [0.0; 4],
            wheel_omega: [0.0; 4],
            wheel_angle: [0.0; 4],
            tires: [TireState::new(self.initial_tire_temp_c); 4],
            abs_factor: [1.0; 4],
            tc_factor: 1.0,
            steer_compliance: 0.0,
            brake_temp_c: [self.initial_tire_temp_c; 4],
            column_angle: 0.0,
            column_rate: 0.0,
            column_accel: 0.0,
            column_primed: false,
        };
        self.drivetrain.reset();
        self.refresh_static_output();
    }

    /// Gives the car a forward speed with matching wheel speeds, for tests
    /// and scripted manoeuvres that start rolling.
    pub fn set_forward_speed(&mut self, speed: f64) {
        self.state.vel = self.state.rot.rotate(Vec3::new(speed, 0.0, 0.0));
        for i in 0..4 {
            let tire = &self.tires[self.corners[i].axle];
            let radius = tire.geometry.unloaded_radius_m
                - (self.corners[i].static_force
                    + self.corners[i].unsprung_mass * qts_core::constants::GRAVITY_M_S2)
                    / tire.vertical.stiffness_n_per_m;
            self.state.wheel_omega[i] = speed / radius;
        }
    }

    /// Sets every tire's thermal nodes to one temperature.
    pub fn set_tire_temperature(&mut self, temp_c: f64) {
        for t in &mut self.state.tires {
            t.thermal = qts_tire::ThermalState::uniform(temp_c);
        }
    }

    fn refresh_static_output(&mut self) {
        self.output = CarOutput::default();
        for i in 0..4 {
            let corner = &self.corners[i];
            let w = &mut self.output.wheels[i];
            w.center_body = (corner.mount + corner.travel_dir * self.state.susp[i]).to_array();
            let t = &self.state.tires[i];
            w.temp_surface_c = t.thermal.surface_c;
            w.temp_core_c = t.thermal.core_c;
            w.temp_carcass_c = t.thermal.carcass_c;
            w.pressure_pa = t.thermal.pressure_pa(&self.tires[corner.axle]);
            w.rolling_radius = self.tires[corner.axle].geometry.unloaded_radius_m;
            w.brake_temp_c = self.state.brake_temp_c[i];
            w.brake_friction_factor = self
                .params
                .brakes
                .friction_factor(self.state.brake_temp_c[i]);
            w.tire_wear = t.wear;
        }
    }

    /// Road-wheel steer angles (left, right) for a steering-wheel angle.
    fn steer_angles(&self, wheel_angle: f64) -> (f64, f64) {
        let s = &self.params.steering;
        let mean = wheel_angle / s.ratio;
        if mean.abs() < 1e-6 {
            return (mean, mean);
        }
        let l = self.params.body.wheelbase_m;
        let half_track = 0.5 * self.params.suspension.front.track_m;
        // Turn radius of the axle centre; positive for a left turn.
        let radius = l / fm::tan(mean);
        let ideal_left = fm::atan(l / (radius - half_track));
        let ideal_right = fm::atan(l / (radius + half_track));
        let k = s.ackermann_fraction;
        (
            mean + k * (ideal_left - mean),
            mean + k * (ideal_right - mean),
        )
    }

    /// Advances the car by one tick.
    pub fn step(
        &mut self,
        controls: &DriverControls,
        assists: &Assists,
        ground: &dyn Ground,
        env: &Environment,
        dt: f64,
    ) {
        let p = &self.params;
        let b = &p.body;
        let rot = self.state.rot;
        let omega = self.state.ang_vel;
        let g_body = rot.inverse_rotate(Vec3::new(0.0, 0.0, -env.gravity));
        let vel_body = rot.inverse_rotate(self.state.vel);
        let m_s = b.sprung_mass_kg;

        let steer = clamp(controls.steer, -1.0, 1.0);
        let steering_wheel_angle = steer * p.steering.max_wheel_angle_rad;
        let (steer_left, steer_right) = self.steer_angles(steering_wheel_angle);

        let mut force = g_body * m_s;
        let mut torque = Vec3::ZERO;

        // Aerodynamics.
        let q = 0.5 * p.aero.air_density_kg_per_m3;
        let air_speed = vel_body.length();
        let drag = vel_body * (-q * p.aero.drag_area_m2 * air_speed);
        force += drag;
        let vx2 = vel_body.x * vel_body.x;
        let lift_front = q * p.aero.lift_area_front_m2 * vx2;
        let lift_rear = q * p.aero.lift_area_rear_m2 * vx2;
        force.z += lift_front + lift_rear;
        torque.y += -b.cg_to_front_axle_m * lift_front + p.cg_to_rear_axle_m() * lift_rear;

        // Body-on-ground penalty contact.
        let mut body_contacts = 0u8;
        for point in self.contact_points {
            let world = self.state.pos + rot.rotate(point);
            let under = ground.sample(world.x, world.y);
            let n = under.normal;
            // Depth below the local ground plane, measured along its normal.
            let penetration = (under.height - world.z) * n.z;
            if penetration <= 0.0 {
                continue;
            }
            body_contacts += 1;
            let v_world = rot.rotate(vel_body + omega.cross(point));
            let normal = (b.contact_stiffness_n_per_m * penetration
                - b.contact_damping_n_s_per_m * v_world.dot(n))
            .max(0.0);
            let tangential = v_world - n * v_world.dot(n);
            let slide = tangential.length();
            let friction = if slide > 1e-9 {
                tangential
                    * (-b.contact_friction * normal * fm::tanh(slide / CONTACT_FRICTION_SPEED)
                        / slide)
            } else {
                Vec3::ZERO
            };
            let f_body = rot.inverse_rotate(friction + n * normal);
            force += f_body;
            torque += point.cross(f_body);
        }

        // Wheels: kinematics, tire load and tire forces.
        let mut rho = [Vec3::ZERO; 4];
        let mut tire_force_body = [Vec3::ZERO; 4];
        let mut wheel_travel_force = [0.0; 4];
        let mut tire_torque = [0.0; 4];
        let mut resist_torque = [0.0; 4];
        let mut wheels = [WheelOutput::default(); 4];
        for i in 0..4 {
            let corner = self.corners[i];
            let ax = axle_of(p, corner.axle);
            let tire = &self.tires[corner.axle];
            let zeta = self.state.susp[i];
            let zeta_rate = self.state.susp_rate[i];
            rho[i] = corner.mount + corner.travel_dir * zeta;
            let center_world = self.state.pos + rot.rotate(rho[i]);
            let sample = ground.sample(center_world.x, center_world.y);
            let unloaded = tire.geometry.unloaded_radius_m;
            let center_vel_body = vel_body + omega.cross(rho[i]) + corner.travel_dir * zeta_rate;
            let center_vel_world = rot.rotate(center_vel_body);
            // The ground under the wheel is treated as a plane through the
            // point below the centre; distances are measured along its normal.
            let n = sample.normal;
            let height_above_ground = (center_world.z - sample.height) * n.z;
            let deflection = unloaded - height_above_ground;
            let load = vertical_load(
                tire,
                &self.state.tires[i],
                deflection,
                -center_vel_world.dot(n),
            );
            let rolling_radius = clamp(height_above_ground, MIN_ROLLING_RADIUS, unloaded);

            // Contact point: the foot of the normal from the wheel centre.
            let contact_world = center_world - n * height_above_ground;
            let contact_body = rot.inverse_rotate(contact_world - self.state.pos);
            let contact_vel_world =
                rot.rotate(vel_body + omega.cross(contact_body) + corner.travel_dir * zeta_rate);

            // Wheel frame in the ground plane.
            let toe = -corner.side * ax.static_toe_rad;
            let steer_angle = match i {
                FL => steer_left + toe + self.state.steer_compliance,
                FR => steer_right + toe + self.state.steer_compliance,
                _ => toe,
            };
            let (sin_d, cos_d) = (fm::sin(steer_angle), fm::cos(steer_angle));
            let heading = rot.rotate(Vec3::new(cos_d, sin_d, 0.0));
            let x_w = (heading - n * heading.dot(n)).normalized_or(Vec3::X);
            let y_w = n.cross(x_w);
            let spin_axis = rot.rotate(Vec3::new(-sin_d, cos_d, 0.0));
            let camber = -fm::asin(clamp(spin_axis.dot(n), -1.0, 1.0))
                + corner.side * (ax.static_camber_rad + ax.camber_gain_rad_per_m * zeta);

            let out = tire_model::step(
                tire,
                &mut self.state.tires[i],
                &TireInput {
                    vx: contact_vel_world.dot(x_w),
                    vy: contact_vel_world.dot(y_w),
                    omega: self.state.wheel_omega[i],
                    fz: load,
                    rolling_radius_m: rolling_radius,
                    camber,
                    surface: sample.surface,
                    air_temp_c: env.air_temp_c,
                    road_temp_c: env.road_temp_c,
                },
                dt,
            );

            let f_world = x_w * out.fx + y_w * out.fy + n * load;
            let f_body = rot.inverse_rotate(f_world);
            let mz_body = rot.inverse_rotate(n * out.mz);
            let weight = g_body * corner.unsprung_mass;
            tire_force_body[i] = f_body;
            wheel_travel_force[i] = corner.travel_dir.dot(f_body + weight);
            force += f_body + weight;
            torque += contact_body.cross(f_body) + rho[i].cross(weight) + mz_body;
            tire_torque[i] = -out.fx * rolling_radius;
            resist_torque[i] = out.rolling_resistance_torque;

            let thermal = &self.state.tires[i].thermal;
            wheels[i] = WheelOutput {
                load,
                fx: out.fx,
                fy: out.fy,
                mz: out.mz,
                slip_ratio: out.slip_ratio,
                slip_angle: out.slip_angle,
                camber,
                steer_angle,
                susp_travel: zeta,
                susp_force: 0.0,
                omega: self.state.wheel_omega[i],
                rolling_radius,
                surface_index: sample.surface_index,
                brake_torque: 0.0,
                drive_torque: 0.0,
                temp_surface_c: thermal.surface_c,
                temp_core_c: thermal.core_c,
                temp_carcass_c: thermal.carcass_c,
                pressure_pa: thermal.pressure_pa(tire),
                brake_temp_c: self.state.brake_temp_c[i],
                brake_friction_factor: p.brakes.friction_factor(self.state.brake_temp_c[i]),
                brake_heat_w: 0.0,
                brake_cooling_w: 0.0,
                tire_wear: self.state.tires[i].wear,
                center_body: rho[i].to_array(),
            };
        }

        // Suspension forces: positive pushes the body up and the wheel down.
        let mut susp_force = [0.0; 4];
        for i in 0..4 {
            let corner = self.corners[i];
            let ax = axle_of(p, corner.axle);
            let zeta = self.state.susp[i];
            let rate = self.state.susp_rate[i];
            let other = self.state.susp[i ^ 1];
            let damping = if rate > 0.0 {
                corner.damp_bump
            } else {
                corner.damp_rebound
            };
            let mut f = corner.static_force + corner.wheel_rate * zeta + damping * rate;
            f += ax.anti_roll_bar_n_per_m * (zeta - other);
            if zeta > ax.bump_travel_m {
                f += ax.bump_stop_rate_n_per_m * (zeta - ax.bump_travel_m);
            } else if zeta < -ax.droop_travel_m {
                f += ax.bump_stop_rate_n_per_m * (zeta + ax.droop_travel_m);
            }
            susp_force[i] = f;
            wheels[i].susp_force = f;
        }

        // Assemble the 10 × 10 system for (a, α, ζ̈).
        let mut a = [[0.0f64; 10]; 10];
        let mut rhs = [0.0f64; 10];
        let mut m_total = m_s;
        let mut rhs_lin = force;
        let i_omega = Vec3::new(
            self.inertia.x * omega.x,
            self.inertia.y * omega.y,
            self.inertia.z * omega.z,
        );
        let mut rhs_ang = torque - omega.cross(i_omega);
        a[3][3] = self.inertia.x;
        a[4][4] = self.inertia.y;
        a[5][5] = self.inertia.z;
        for i in 0..4 {
            let m = self.corners[i].unsprung_mass;
            let r = rho[i];
            m_total += m;
            // Linear-angular coupling: −m [r]×.
            a[0][4] += m * r.z;
            a[0][5] -= m * r.y;
            a[1][3] -= m * r.z;
            a[1][5] += m * r.x;
            a[2][3] += m * r.y;
            a[2][4] -= m * r.x;
            // Angular block: m (|r|² I − r rᵀ).
            let r2 = r.length_sq();
            let rv = [r.x, r.y, r.z];
            for row in 0..3 {
                for col in 0..3 {
                    let identity = if row == col { r2 } else { 0.0 };
                    a[3 + row][3 + col] += m * (identity - rv[row] * rv[col]);
                }
            }
            // Travel column: the wheel moves along `d` per unit travel.
            let d = self.corners[i].travel_dir;
            let rxd = r.cross(d);
            a[0][6 + i] = m * d.x;
            a[1][6 + i] = m * d.y;
            a[2][6 + i] = m * d.z;
            a[3][6 + i] = m * rxd.x;
            a[4][6 + i] = m * rxd.y;
            a[5][6 + i] = m * rxd.z;
            a[6 + i][6 + i] = m * d.length_sq();
            // Velocity-dependent terms.
            let bias =
                omega.cross(omega.cross(r)) + omega.cross(d) * (2.0 * self.state.susp_rate[i]);
            rhs_lin -= bias * m;
            rhs_ang -= r.cross(bias) * m;
            rhs[6 + i] = wheel_travel_force[i] - susp_force[i] - m * d.dot(bias);
        }
        a[0][0] = m_total;
        a[1][1] = m_total;
        a[2][2] = m_total;
        // Mirror the upper triangle.
        for row in 0..10 {
            for col in (row + 1)..10 {
                a[col][row] = a[row][col];
            }
        }
        rhs[0] = rhs_lin.x;
        rhs[1] = rhs_lin.y;
        rhs[2] = rhs_lin.z;
        rhs[3] = rhs_ang.x;
        rhs[4] = rhs_ang.y;
        rhs[5] = rhs_ang.z;
        let solved = solve_spd(&mut a, &mut rhs);
        let (accel_body, ang_accel, travel_accel) = if solved {
            (
                Vec3::new(rhs[0], rhs[1], rhs[2]),
                Vec3::new(rhs[3], rhs[4], rhs[5]),
                [rhs[6], rhs[7], rhs[8], rhs[9]],
            )
        } else {
            // The mass matrix is positive definite for any valid parameter
            // set, so this branch is unreachable in practice. Freezing the
            // accelerations keeps the state finite if it is ever reached.
            (Vec3::ZERO, Vec3::ZERO, [0.0; 4])
        };

        // Anti-lock modulation from this tick's slip.
        let brake = clamp(controls.brake, 0.0, 1.0);
        let abs = &p.brakes.abs;
        let mut abs_active = [false; 4];
        for i in 0..4 {
            let f = &mut self.state.abs_factor[i];
            if !abs.enabled || brake <= ABS_PEDAL_THRESHOLD {
                *f = 1.0;
                continue;
            }
            let moving = vel_body.x.abs() > abs.min_speed_m_per_s;
            if moving {
                // Steer the pressure toward the target slip: release in
                // proportion to the excess, reapply in proportion to the
                // shortfall, both rate-limited.
                let braking_slip = -wheels[i].slip_ratio;
                let rate = clamp(
                    abs.control_gain_per_s * (abs.slip_threshold - braking_slip),
                    -abs.release_rate_per_s,
                    abs.apply_rate_per_s,
                );
                *f += rate * dt;
            } else {
                *f += abs.apply_rate_per_s * dt;
            }
            *f = clamp(*f, ABS_MIN_FACTOR, 1.0);
            abs_active[i] = *f < 0.999;
        }
        let handbrake = clamp(controls.handbrake, 0.0, 1.0);
        let mut service_brake = [0.0; 4];
        let mut handbrake_torque = [0.0; 4];
        let mut wheel_inertia = [0.0; 4];
        for i in 0..4 {
            let max = if i < 2 {
                p.brakes.max_torque_front_nm
            } else {
                p.brakes.max_torque_rear_nm
            };
            // Pad friction follows the disc temperature: fade when hot.
            let fade = p.brakes.friction_factor(self.state.brake_temp_c[i]);
            service_brake[i] = brake * max * self.state.abs_factor[i] * fade;
            if i >= 2 {
                handbrake_torque[i] = handbrake * p.brakes.handbrake_torque_nm * fade;
            }
            wheel_inertia[i] = self.corners[i].wheel_inertia;
        }

        // Traction control from this tick's slip: one torque fraction for
        // the whole car, steered toward the target drive slip.
        let pedal = clamp(controls.throttle, 0.0, 1.0);
        let tc = &p.traction_control;
        if tc.enabled && assists.traction_control && pedal > TC_PEDAL_THRESHOLD {
            let reference = vel_body.x.abs().max(tc.min_speed_m_per_s);
            let mut slip: f64 = 0.0;
            for i in 0..4 {
                let rolling = self.state.wheel_omega[i].abs() * wheels[i].rolling_radius;
                slip = slip.max((rolling - vel_body.x.abs()) / reference);
            }
            let rate = clamp(
                tc.control_gain_per_s * (tc.slip_threshold - slip),
                -tc.release_rate_per_s,
                tc.apply_rate_per_s,
            );
            self.state.tc_factor = clamp(self.state.tc_factor + rate * dt, TC_MIN_FACTOR, 1.0);
        } else {
            self.state.tc_factor = 1.0;
        }
        // The cut never takes the demand below the pedal travel where
        // lift-off regeneration or engine braking would begin.
        let throttle = (pedal * self.state.tc_factor).max(pedal.min(TC_PEDAL_FLOOR));

        // Driveline and wheel spin.
        let drive = self.drivetrain.step(
            &DriveContext {
                controls: DriveControls {
                    throttle,
                    clutch: clamp(controls.clutch, 0.0, 1.0),
                    shift_up: controls.shift_up,
                    shift_down: controls.shift_down,
                    gear_request: controls.gear_request,
                    auto_clutch: assists.auto_clutch,
                    auto_shift: assists.auto_shift,
                    starter: controls.starter,
                    engine_damage: assists.engine_damage,
                },
                wheel_omega: self.state.wheel_omega,
                wheel_inertia,
                tire_torque,
                resist_torque,
                service_brake_torque: service_brake,
                handbrake_torque,
                abs_active,
                speed: vel_body.x,
                yaw_rate: omega.z,
                steer,
            },
            dt,
        );

        // Steering-column motion, from the driver's wheel.
        let st = &p.steering;
        {
            let s = &mut self.state;
            if !s.column_primed {
                s.column_angle = steering_wheel_angle;
                s.column_primed = true;
            }
            let alpha = dt / (1.0 / (std::f64::consts::TAU * COLUMN_FILTER_HZ) + dt);
            let raw_rate = (steering_wheel_angle - s.column_angle) / dt;
            let prev_rate = s.column_rate;
            s.column_rate += alpha * (raw_rate - s.column_rate);
            s.column_accel += alpha * ((s.column_rate - prev_rate) / dt - s.column_accel);
            s.column_angle = steering_wheel_angle;
        }

        // Moments about the kingpin axes (positive turning the wheels left).
        let legacy = self.legacy_steering;
        let mut kingpin_moment = 0.0;
        let mut weight_moment = 0.0;
        let mut gyro_moment = 0.0;
        for i in [FL, FR] {
            let corner = &self.corners[i];
            let side = corner.side;
            let d = wheels[i].steer_angle;
            let (sin_d, cos_d) = (fm::sin(d), fm::cos(d));
            let r0 = Vec3::new(-st.mechanical_trail_m, side * st.scrub_radius_m, 0.0);
            let r = Vec3::new(
                r0.x * cos_d - r0.y * sin_d,
                r0.x * sin_d + r0.y * cos_d,
                0.0,
            );
            let axis = Vec3::new(
                -fm::tan(st.caster_rad),
                -side * fm::tan(st.kingpin_inclination_rad),
                1.0,
            )
            .normalized_or(Vec3::Z);
            if legacy.is_some() {
                // The model before D-075, kept for comparison only.
                kingpin_moment += axis.dot(r.cross(tire_force_body[i])) + axis.z * wheels[i].mz;
                continue;
            }
            // Tire forces in the ground plane through the trail and scrub
            // lever, plus the tire's own aligning moment.
            let f = tire_force_body[i];
            let horizontal = Vec3::new(f.x, f.y, 0.0);
            kingpin_moment += axis.dot(r.cross(horizontal)) + axis.z * wheels[i].mz;
            // Vertical load through kingpin inclination (lifting the car,
            // centring) and caster (weight transfer across the axle,
            // centring in a turn): Gillespie, Fundamentals of Vehicle
            // Dynamics (1992), chapter 8, with the lateral offset at the
            // ground as the scrub radius.
            let fz = wheels[i].load;
            let weight = -fz * st.scrub_radius_m * fm::sin(st.kingpin_inclination_rad) * sin_d
                + side * fz * st.scrub_radius_m * fm::sin(st.caster_rad) * cos_d;
            weight_moment += weight;
            // Gyroscopic moment: the spinning wheel's angular momentum
            // turned by the body's rotation and by camber change from the
            // suspension. The steering must supply (Ω × H) to the wheel;
            // the wheel pushes back on it with the opposite moment.
            let spin = corner.wheel_inertia * self.state.wheel_omega[i];
            let h = Vec3::new(-sin_d, cos_d, 0.0) * spin;
            let camber_rate =
                -side * p.suspension.front.camber_gain_rad_per_m * self.state.susp_rate[i];
            let carrier = omega + Vec3::new(cos_d, sin_d, 0.0) * camber_rate;
            let gyro = -axis.dot(carrier.cross(h));
            gyro_moment += gyro;
            kingpin_moment += weight + gyro;
        }
        let load_nm = kingpin_moment / st.ratio;
        let speed_now = vel_body.x;
        let (driver_nm, assist_gain, friction_nm, inertia_nm) = match legacy {
            Some(fraction) => (load_nm * (1.0 - fraction), 0.0, 0.0, 0.0),
            None => {
                // Friction against the direction the driver turns, and the
                // reflected inertia against the column's acceleration.
                let s = &self.state;
                let friction =
                    -st.rack_friction_nm * fm::tanh(s.column_rate / RACK_FRICTION_SMOOTHING_RAD_S);
                let inertia = -st.steering_inertia_kg_m2 * s.column_accel;
                (
                    st.driver_torque(load_nm, speed_now),
                    st.assist_gain_at(speed_now),
                    friction,
                    inertia,
                )
            }
        };
        let steering_torque_nm = driver_nm + friction_nm + inertia_nm;
        let steering = SteeringOutput {
            load_nm,
            weight_nm: weight_moment / st.ratio,
            gyro_nm: gyro_moment / st.ratio,
            assist_nm: driver_nm - load_nm,
            friction_nm,
            inertia_nm,
            assist_gain,
        };
        // The steering system twists under the kingpin moment, taking a
        // little steer angle off. A short lag keeps this out of the same
        // tick it was computed from.
        let compliance_target = st.compliance_rad_per_nm * kingpin_moment;
        self.state.steer_compliance += (compliance_target - self.state.steer_compliance)
            * (dt / (STEER_COMPLIANCE_LAG_S + dt));

        // Integrate: velocities first, then positions from the new
        // velocities.
        self.state.vel += rot.rotate(accel_body) * dt;
        self.state.ang_vel += ang_accel * dt;
        for i in 0..4 {
            self.state.susp_rate[i] += travel_accel[i] * dt;
            self.state.susp[i] += self.state.susp_rate[i] * dt;
            self.state.wheel_omega[i] = drive.wheel_omega[i];
            let mut angle = self.state.wheel_angle[i] + drive.wheel_omega[i] * dt;
            if angle > std::f64::consts::PI {
                angle -= 2.0 * std::f64::consts::PI;
            } else if angle < -std::f64::consts::PI {
                angle += 2.0 * std::f64::consts::PI;
            }
            self.state.wheel_angle[i] = angle;
            wheels[i].brake_torque = drive.brake_torque[i];
            wheels[i].drive_torque = drive.shaft_torque[i];
            // Brake disc heat: the friction work of the brake, less the
            // pads' share, against convection and radiation.
            let th = if i < 2 {
                &p.brakes.thermal_front
            } else {
                &p.brakes.thermal_rear
            };
            let heat = th.disc_heat_share * (drive.brake_torque[i] * drive.wheel_omega[i]).abs();
            let temp = self.state.brake_temp_c[i];
            let cooling = th.cooling_w(temp, env.air_temp_c, vel_body.x);
            self.state.brake_temp_c[i] = temp + dt * (heat - cooling) / th.heat_capacity_j_per_k;
            wheels[i].brake_temp_c = self.state.brake_temp_c[i];
            wheels[i].brake_friction_factor = p.brakes.friction_factor(temp);
            wheels[i].brake_heat_w = heat;
            wheels[i].brake_cooling_w = cooling;
            wheels[i].tire_wear = self.state.tires[i].wear;
        }
        self.state.pos += self.state.vel * dt;
        self.state.rot = rot.integrate_body_rate(self.state.ang_vel, dt);

        let ground_vel = Vec3::new(self.state.vel.x, self.state.vel.y, 0.0);
        self.output = CarOutput {
            wheels,
            steering_torque_nm,
            steering_wheel_angle,
            steering,
            accel_body: accel_body.to_array(),
            vel_body: vel_body.to_array(),
            speed: ground_vel.length(),
            yaw_rate: self.state.ang_vel.z,
            drive: drive.telemetry,
            aero_drag_n: drag.length(),
            body_contacts,
        };
    }

    /// Mechanical energy of the car, J: kinetic energy of the body, the
    /// unsprung masses, the wheels and the drivetrain, plus gravitational
    /// and elastic potential energy (springs, anti-roll bars, travel stops
    /// and tire vertical springs). The small energy stored in tire carcass
    /// deflection is left out: the deflection state only behaves as a spring
    /// while the tire is not sliding.
    ///
    /// `ground` supplies the height under each wheel for the tire spring
    /// term. Used by the energy sanity tests.
    pub fn mechanical_energy(&self, ground: &dyn Ground, gravity: f64) -> f64 {
        let s = &self.state;
        let p = &self.params;
        let vel_body = s.rot.inverse_rotate(s.vel);
        let w = s.ang_vel;
        let mut e = 0.5 * p.body.sprung_mass_kg * s.vel.length_sq()
            + 0.5
                * (self.inertia.x * w.x * w.x
                    + self.inertia.y * w.y * w.y
                    + self.inertia.z * w.z * w.z)
            + p.body.sprung_mass_kg * gravity * s.pos.z;
        for i in 0..4 {
            let c = &self.corners[i];
            let ax = axle_of(p, c.axle);
            let tire = &self.tires[c.axle];
            let rho = c.mount + c.travel_dir * s.susp[i];
            let center = s.pos + s.rot.rotate(rho);
            let v = vel_body + w.cross(rho) + c.travel_dir * s.susp_rate[i];
            e += 0.5 * c.unsprung_mass * v.length_sq();
            e += 0.5 * c.wheel_inertia * s.wheel_omega[i] * s.wheel_omega[i];
            e += c.unsprung_mass * gravity * center.z;
            let z = s.susp[i];
            e += c.static_force * z + 0.5 * c.wheel_rate * z * z;
            if z > ax.bump_travel_m {
                e += 0.5 * ax.bump_stop_rate_n_per_m * (z - ax.bump_travel_m).powi(2);
            } else if z < -ax.droop_travel_m {
                e += 0.5 * ax.bump_stop_rate_n_per_m * (z + ax.droop_travel_m).powi(2);
            }
            if i % 2 == 0 {
                let diff = z - s.susp[i + 1];
                e += 0.5 * ax.anti_roll_bar_n_per_m * diff * diff;
            }
            let under = ground.sample(center.x, center.y);
            let deflection =
                tire.geometry.unloaded_radius_m - (center.z - under.height) * under.normal.z;
            if deflection > 0.0 {
                e += 0.5 * vertical_stiffness(tire, &s.tires[i]) * deflection * deflection;
            }
        }
        e + self.drivetrain.kinetic_energy()
    }

    /// True when every state value is finite.
    pub fn is_finite(&self) -> bool {
        let s = &self.state;
        s.pos.is_finite()
            && s.rot.is_finite()
            && s.vel.is_finite()
            && s.ang_vel.is_finite()
            && s.susp.iter().all(|v| v.is_finite())
            && s.susp_rate.iter().all(|v| v.is_finite())
            && s.wheel_omega.iter().all(|v| v.is_finite())
            && s.tires.iter().all(|t| t.is_finite())
    }
}

impl Snapshot for Car {
    fn save(&self, w: &mut StateWriter) {
        let s = &self.state;
        w.write_vec3(s.pos);
        w.write_quat(s.rot);
        w.write_vec3(s.vel);
        w.write_vec3(s.ang_vel);
        w.write_f64s(&s.susp);
        w.write_f64s(&s.susp_rate);
        w.write_f64s(&s.wheel_omega);
        w.write_f64s(&s.wheel_angle);
        for t in &s.tires {
            t.save(w);
        }
        w.write_f64s(&s.abs_factor);
        w.write_f64(s.tc_factor);
        w.write_f64(s.steer_compliance);
        w.write_f64s(&s.brake_temp_c);
        w.write_f64(s.column_angle);
        w.write_f64(s.column_rate);
        w.write_f64(s.column_accel);
        w.write_bool(s.column_primed);
        self.drivetrain.save(w);
    }

    fn load(&mut self, r: &mut StateReader<'_>) -> Result<(), SnapshotError> {
        let s = &mut self.state;
        s.pos = r.read_vec3()?;
        s.rot = r.read_quat()?;
        s.vel = r.read_vec3()?;
        s.ang_vel = r.read_vec3()?;
        r.read_f64s(&mut s.susp)?;
        r.read_f64s(&mut s.susp_rate)?;
        r.read_f64s(&mut s.wheel_omega)?;
        r.read_f64s(&mut s.wheel_angle)?;
        for t in &mut s.tires {
            t.load(r)?;
        }
        r.read_f64s(&mut s.abs_factor)?;
        s.tc_factor = r.read_f64()?;
        s.steer_compliance = r.read_f64()?;
        r.read_f64s(&mut s.brake_temp_c)?;
        s.column_angle = r.read_f64()?;
        s.column_rate = r.read_f64()?;
        s.column_accel = r.read_f64()?;
        s.column_primed = r.read_bool()?;
        self.drivetrain.load(r)?;
        self.refresh_static_output();
        Ok(())
    }
}
