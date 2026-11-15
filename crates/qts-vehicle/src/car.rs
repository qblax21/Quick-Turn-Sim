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
}

/// Driver assists, fixed for a session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Assists {
    /// The clutch engages and releases by itself.
    pub auto_clutch: bool,
    /// Gears are selected automatically.
    pub auto_shift: bool,
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
            let ground_height = ground.sample(world.x, world.y).height;
            let penetration = ground_height - world.z;
            if penetration <= 0.0 {
                continue;
            }
            body_contacts += 1;
            let v_world = rot.rotate(vel_body + omega.cross(point));
            let normal = (b.contact_stiffness_n_per_m * penetration
                - b.contact_damping_n_s_per_m * v_world.z)
                .max(0.0);
            let tangential = Vec3::new(v_world.x, v_world.y, 0.0);
            let slide = tangential.length();
            let friction = if slide > 1e-9 {
                tangential
                    * (-b.contact_friction * normal * fm::tanh(slide / CONTACT_FRICTION_SPEED)
                        / slide)
            } else {
                Vec3::ZERO
            };
            let f_body = rot.inverse_rotate(Vec3::new(friction.x, friction.y, normal));
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
            let height_above_ground = center_world.z - sample.height;
            let deflection = unloaded - height_above_ground;
            let load = vertical_load(tire, &self.state.tires[i], deflection, -center_vel_world.z);
            let rolling_radius = clamp(height_above_ground, MIN_ROLLING_RADIUS, unloaded);

            // Contact point directly below the wheel centre.
            let contact_world = Vec3::new(center_world.x, center_world.y, sample.height);
            let contact_body = rot.inverse_rotate(contact_world - self.state.pos);
            let contact_vel_world =
                rot.rotate(vel_body + omega.cross(contact_body) + corner.travel_dir * zeta_rate);

            // Wheel frame in the ground plane.
            let toe = -corner.side * ax.static_toe_rad;
            let steer_angle = match i {
                FL => steer_left + toe,
                FR => steer_right + toe,
                _ => toe,
            };
            let (sin_d, cos_d) = (fm::sin(steer_angle), fm::cos(steer_angle));
            let heading = rot.rotate(Vec3::new(cos_d, sin_d, 0.0));
            let x_w = Vec3::new(heading.x, heading.y, 0.0).normalized_or(Vec3::X);
            let y_w = Vec3::new(-x_w.y, x_w.x, 0.0);
            let spin_axis = rot.rotate(Vec3::new(-sin_d, cos_d, 0.0));
            let camber = -fm::asin(clamp(spin_axis.z, -1.0, 1.0))
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

            let f_world = x_w * out.fx + y_w * out.fy + Vec3::Z * load;
            let f_body = rot.inverse_rotate(f_world);
            let mz_body = rot.inverse_rotate(Vec3::Z * out.mz);
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
            if moving && wheels[i].slip_ratio < -abs.slip_threshold {
                *f -= abs.release_rate_per_s * dt;
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
            service_brake[i] = brake * max * self.state.abs_factor[i];
            if i >= 2 {
                handbrake_torque[i] = handbrake * p.brakes.handbrake_torque_nm;
            }
            wheel_inertia[i] = self.corners[i].wheel_inertia;
        }

        // Driveline and wheel spin.
        let drive = self.drivetrain.step(
            &DriveContext {
                controls: DriveControls {
                    throttle: clamp(controls.throttle, 0.0, 1.0),
                    clutch: clamp(controls.clutch, 0.0, 1.0),
                    shift_up: controls.shift_up,
                    shift_down: controls.shift_down,
                    gear_request: controls.gear_request,
                    auto_clutch: assists.auto_clutch,
                    auto_shift: assists.auto_shift,
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

        // Steering torque about the kingpin axes, referred to the column.
        let st = &p.steering;
        let mut kingpin_moment = 0.0;
        for i in [FL, FR] {
            let side = self.corners[i].side;
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
            kingpin_moment += axis.dot(r.cross(tire_force_body[i])) + axis.z * wheels[i].mz;
        }
        let steering_torque_nm = kingpin_moment / st.ratio;

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
        }
        self.state.pos += self.state.vel * dt;
        self.state.rot = rot.integrate_body_rate(self.state.ang_vel, dt);

        let ground_vel = Vec3::new(self.state.vel.x, self.state.vel.y, 0.0);
        self.output = CarOutput {
            wheels,
            steering_torque_nm,
            steering_wheel_angle,
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
            let height = ground.sample(center.x, center.y).height;
            let deflection = tire.geometry.unloaded_radius_m - (center.z - height);
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
        self.drivetrain.load(r)?;
        self.refresh_static_output();
        Ok(())
    }
}
