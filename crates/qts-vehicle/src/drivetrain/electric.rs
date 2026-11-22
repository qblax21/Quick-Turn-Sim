// SPDX-License-Identifier: Apache-2.0
//! Dual-motor electric all-wheel drive: one motor per axle through a fixed
//! reduction and an open differential, a torque-vectoring hook, and
//! regenerative braking blended with the friction brakes.

use super::{brake_share, DriveContext, DriveResult, DriveTelemetry, Drivetrain, RAD_S_TO_RPM};
use crate::params::{DualMotorParams, MotorParams, TorqueVectoringKind};
use crate::spin::SpinSolver;
use qts_core::math::{clamp, sign, smoothstep};
use qts_core::snapshot::{Snapshot, SnapshotError, StateReader, StateWriter};

/// Gauss-Seidel sweeps per tick. Higher than the engine module because the
/// gear meshes are rigid constraints sharing wheels with the brakes.
const SOLVER_ITERATIONS: u32 = 16;
/// Accelerator travel over which lift-off regeneration fades out.
const COAST_REGEN_PEDAL_SPAN: f64 = 0.1;
/// Rotor speed floor when turning a power limit into a torque limit, rad/s.
const POWER_LIMIT_SPEED_FLOOR: f64 = 1.0;
/// Speed below which the drive direction may be changed, m/s.
const DIRECTION_SELECT_SPEED: f64 = 2.0;

/// What a torque-vectoring strategy is told each tick.
#[derive(Clone, Copy, Debug)]
pub struct TvContext {
    /// Accelerator, 0 to 1.
    pub throttle: f64,
    /// Steering input, -1 to 1, positive left.
    pub steer: f64,
    /// Vehicle speed along its x axis, m/s.
    pub speed: f64,
    /// Yaw rate, rad/s.
    pub yaw_rate: f64,
    /// Wheel speeds, rad/s.
    pub wheel_omega: [f64; 4],
    /// The configured base front torque share.
    pub base_front_share: f64,
}

/// What a torque-vectoring strategy asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TvCommand {
    /// Share of drive torque for the front axle, 0 to 1.
    pub front_share: f64,
    /// Extra friction-brake torque per wheel, N·m (zero or positive). With
    /// open differentials this is how a strategy steers torque left or
    /// right.
    pub brake_torque: [f64; 4],
}

/// The torque-vectoring hook. A strategy must be a pure function of its
/// context so the simulation stays deterministic.
pub trait TorqueVectoring: Send {
    /// Chooses the front/rear split and any per-wheel brake intervention.
    fn allocate(&self, ctx: &TvContext) -> TvCommand;
}

/// The default strategy: the configured split, no intervention.
#[derive(Clone, Copy, Debug, Default)]
pub struct FixedSplit;

impl TorqueVectoring for FixedSplit {
    fn allocate(&self, ctx: &TvContext) -> TvCommand {
        TvCommand {
            front_share: ctx.base_front_share,
            brake_torque: [0.0; 4],
        }
    }
}

/// The dual-motor electric drivetrain module.
pub struct DualMotor {
    p: DualMotorParams,
    tv: Box<dyn TorqueVectoring>,
    motor_omega: [f64; 2],
    gear: i8,
    battery_energy_j: f64,
}

impl std::fmt::Debug for DualMotor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DualMotor")
            .field("motor_omega", &self.motor_omega)
            .field("gear", &self.gear)
            .field("battery_energy_j", &self.battery_energy_j)
            .finish_non_exhaustive()
    }
}

fn drive_limit(m: &MotorParams, rotor_speed: f64) -> f64 {
    let w = rotor_speed.abs();
    if w >= m.max_speed_rad_per_s {
        return 0.0;
    }
    let by_power = m.peak_power_w / w.max(POWER_LIMIT_SPEED_FLOOR);
    // Taper to zero over the last two percent of the speed range.
    let taper = clamp(
        (m.max_speed_rad_per_s - w) / (0.02 * m.max_speed_rad_per_s),
        0.0,
        1.0,
    );
    m.peak_torque_nm.min(by_power) * taper
}

fn regen_limit(m: &MotorParams, rotor_speed: f64) -> f64 {
    let w = rotor_speed.abs();
    let by_power = m.regen_power_limit_w / w.max(POWER_LIMIT_SPEED_FLOOR);
    m.regen_torque_limit_nm.min(by_power)
}

impl DualMotor {
    /// Builds the module at rest in drive with the configured charge.
    pub fn new(p: DualMotorParams) -> Self {
        let tv: Box<dyn TorqueVectoring> = match p.torque_vectoring {
            TorqueVectoringKind::None => Box::new(FixedSplit),
        };
        let battery_energy_j = p.battery.capacity_j * p.battery.initial_soc;
        Self {
            p,
            tv,
            motor_omega: [0.0; 2],
            gear: 1,
            battery_energy_j,
        }
    }

    /// Replaces the torque-vectoring strategy.
    pub fn set_torque_vectoring(&mut self, tv: Box<dyn TorqueVectoring>) {
        self.tv = tv;
    }

    /// Energy currently stored in the battery, J.
    pub fn battery_energy_j(&self) -> f64 {
        self.battery_energy_j
    }

    fn handle_direction(&mut self, ctx: &DriveContext) {
        let c = &ctx.controls;
        let slow = ctx.speed.abs() < DIRECTION_SELECT_SPEED;
        let mut want = self.gear;
        if let Some(g) = c.gear_request {
            want = g.clamp(-1, 1);
        } else if c.shift_up {
            want = (self.gear + 1).min(1);
        } else if c.shift_down {
            want = (self.gear - 1).max(-1);
        } else if c.auto_shift && self.gear == 0 && c.throttle > 0.05 {
            want = 1;
        }
        // Neutral is always available; a direction only when nearly stopped.
        if want != self.gear && (want == 0 || slow) {
            self.gear = want;
        }
    }
}

impl Drivetrain for DualMotor {
    fn step(&mut self, ctx: &DriveContext, dt: f64) -> DriveResult {
        self.handle_direction(ctx);
        let c = &ctx.controls;
        let motors = [&self.p.front_motor, &self.p.rear_motor];
        let axle_wheels = [[0usize, 1usize], [2usize, 3usize]];
        let throttle = clamp(c.throttle, 0.0, 1.0);
        let direction = f64::from(self.gear);
        let soc = self.battery_energy_j / self.p.battery.capacity_j;

        // Rotor speeds follow the wheels through the reduction.
        let mut rotor = [0.0; 2];
        for a in 0..2 {
            let [l, r] = axle_wheels[a];
            rotor[a] = motors[a].reduction_ratio * 0.5 * (ctx.wheel_omega[l] + ctx.wheel_omega[r]);
        }

        // Torque limits at the rotors.
        let fade = smoothstep(
            (ctx.speed.abs() - self.p.regen.fade_low_m_per_s)
                / (self.p.regen.fade_high_m_per_s - self.p.regen.fade_low_m_per_s),
        );
        let mut drive_cap = [0.0; 2];
        let mut regen_cap = [0.0; 2];
        for a in 0..2 {
            drive_cap[a] = if soc > 0.0 && self.gear != 0 {
                drive_limit(motors[a], rotor[a])
            } else {
                0.0
            };
            regen_cap[a] = if soc < 1.0 && self.gear != 0 {
                regen_limit(motors[a], rotor[a]) * fade
            } else {
                0.0
            };
        }
        // Battery charge limit: scale both regeneration limits together so
        // the friction brakes are sized against what can really be absorbed.
        let potential_charge: f64 = (0..2)
            .map(|a| regen_cap[a] * rotor[a].abs() * motors[a].efficiency)
            .sum();
        if potential_charge > self.p.battery.max_charge_power_w && potential_charge > 0.0 {
            let k = self.p.battery.max_charge_power_w / potential_charge;
            regen_cap[0] *= k;
            regen_cap[1] *= k;
        }

        // Drive demand, split between the axles by the vectoring hook.
        let tv = self.tv.allocate(&TvContext {
            throttle,
            steer: ctx.steer,
            speed: ctx.speed,
            yaw_rate: ctx.yaw_rate,
            wheel_omega: ctx.wheel_omega,
            base_front_share: self.p.front_torque_share,
        });
        let share = clamp(tv.front_share, 0.0, 1.0);
        let wheel_cap = [
            drive_cap[0] * motors[0].reduction_ratio,
            drive_cap[1] * motors[1].reduction_ratio,
        ];
        let total = throttle * (wheel_cap[0] + wheel_cap[1]);
        let mut demand = [0.0; 2];
        demand[0] = (total * share).min(wheel_cap[0]);
        demand[1] = (total - demand[0]).min(wheel_cap[1]);
        demand[0] = (total - demand[1]).min(wheel_cap[0]);

        // Regeneration: lift-off first, then a share of the brake demand.
        let coast = self.p.regen.coast_fraction
            * (1.0 - clamp(throttle / COAST_REGEN_PEDAL_SPAN, 0.0, 1.0));
        let intervening = ctx.abs_active.iter().any(|&x| x);
        // Lift-off regeneration, shared between the axles in proportion to
        // the load each carries, as a brake system's bias shares braking,
        // so the lighter end is not over-braked (D-088). Each axle takes no
        // more than its motor can; the other makes up what it can.
        let cap_wheel = [
            regen_cap[0] * motors[0].reduction_ratio,
            regen_cap[1] * motors[1].reduction_ratio,
        ];
        let axle_load = |a: usize| {
            let [l, r] = axle_wheels[a];
            ctx.wheel_load[l].max(0.0) + ctx.wheel_load[r].max(0.0)
        };
        let loads = [axle_load(0), axle_load(1)];
        let front_part = if loads[0] + loads[1] > 1.0 {
            loads[0] / (loads[0] + loads[1])
        } else {
            0.5
        };
        let coast_total = coast * (cap_wheel[0] + cap_wheel[1]);
        let mut coast_wheel = [0.0; 2];
        coast_wheel[0] = (coast_total * front_part).min(cap_wheel[0]);
        coast_wheel[1] = (coast_total - coast_wheel[0]).min(cap_wheel[1]);
        coast_wheel[0] = (coast_total - coast_wheel[1]).min(cap_wheel[0]);
        let mut friction_cap = [0.0; 4];
        let mut regen_rotor = [0.0; 2];
        for a in 0..2 {
            let [l, r] = axle_wheels[a];
            let ratio = motors[a].reduction_ratio;
            // Regeneration stands down completely while the anti-lock
            // system is working anywhere, so it cannot fight the modulation.
            let coast_rotor = if intervening {
                0.0
            } else {
                coast_wheel[a] / ratio
            };
            let service = ctx.service_brake_torque[l] + ctx.service_brake_torque[r];
            let handbrake = ctx.handbrake_torque[l] + ctx.handbrake_torque[r] > 0.0;
            let blend_wheel = if intervening || handbrake {
                0.0
            } else {
                (service * self.p.regen.brake_share).min((regen_cap[a] - coast_rotor) * ratio)
            }
            .max(0.0);
            regen_rotor[a] = coast_rotor + blend_wheel / ratio;
            for w in [l, r] {
                friction_cap[w] = (ctx.service_brake_torque[w] - 0.5 * blend_wheel).max(0.0)
                    + ctx.handbrake_torque[w]
                    + tv.brake_torque[w].max(0.0);
            }
        }

        // Rotor torque commands, then the battery discharge limit.
        let mut torque = [0.0; 2];
        let mut drive_torque = [0.0; 2];
        for a in 0..2 {
            drive_torque[a] = direction * demand[a] / motors[a].reduction_ratio;
        }
        let discharge: f64 = (0..2)
            .map(|a| (drive_torque[a] * rotor[a]).max(0.0) / motors[a].efficiency)
            .sum();
        let discharge_scale = if discharge > self.p.battery.max_discharge_power_w {
            self.p.battery.max_discharge_power_w / discharge
        } else {
            1.0
        };
        for a in 0..2 {
            torque[a] = drive_torque[a] * discharge_scale - sign(rotor[a]) * regen_rotor[a];
        }

        // Coupled solve: four wheels plus two rotors.
        const FRONT_ROTOR: usize = 4;
        let omega = [
            ctx.wheel_omega[0],
            ctx.wheel_omega[1],
            ctx.wheel_omega[2],
            ctx.wheel_omega[3],
            rotor[0],
            rotor[1],
        ];
        let inertia = [
            ctx.wheel_inertia[0],
            ctx.wheel_inertia[1],
            ctx.wheel_inertia[2],
            ctx.wheel_inertia[3],
            motors[0].rotor_inertia_kg_m2,
            motors[1].rotor_inertia_kg_m2,
        ];
        let mut solver = SpinSolver::new(&omega, &inertia, dt);
        for i in 0..4 {
            solver.apply_torque(i, ctx.tire_torque[i]);
        }
        solver.apply_torque(FRONT_ROTOR, torque[0]);
        solver.apply_torque(FRONT_ROTOR + 1, torque[1]);
        let mut friction = [None; 4];
        for i in 0..4 {
            friction[i] =
                solver.add_constraint(&[(i, 1.0)], friction_cap[i] + ctx.resist_torque[i]);
        }
        let mut mesh = [None; 2];
        for a in 0..2 {
            let [l, r] = axle_wheels[a];
            let ratio = motors[a].reduction_ratio;
            mesh[a] = solver.add_constraint(
                &[(FRONT_ROTOR + a, 1.0), (l, -0.5 * ratio), (r, -0.5 * ratio)],
                f64::INFINITY,
            );
        }
        solver.solve(SOLVER_ITERATIONS);

        let mut result = DriveResult::default();
        for i in 0..4 {
            result.wheel_omega[i] = solver.omega(i);
            if let Some(idx) = friction[i] {
                result.brake_torque[i] = brake_share(
                    solver.constraint_torque(idx),
                    friction_cap[i],
                    ctx.resist_torque[i],
                );
            }
        }
        let mut drive_power = 0.0;
        let mut regen_power = 0.0;
        let mut battery_power = 0.0;
        for a in 0..2 {
            let [l, r] = axle_wheels[a];
            let ratio = motors[a].reduction_ratio;
            // Remove the small residual the iterative solve leaves in the
            // mesh by slaving the rotor to the wheels.
            let carrier = 0.5 * (result.wheel_omega[l] + result.wheel_omega[r]);
            self.motor_omega[a] = ratio * carrier;
            // Torque the mesh delivered to the axle.
            let axle_torque = mesh[a].map_or(0.0, |idx| -solver.constraint_torque(idx) * ratio);
            result.shaft_torque[l] = 0.5 * axle_torque;
            result.shaft_torque[r] = 0.5 * axle_torque;
            let mech = torque[a] * 0.5 * (rotor[a] + self.motor_omega[a]);
            if mech >= 0.0 {
                drive_power += mech;
                battery_power += mech / motors[a].efficiency;
            } else {
                regen_power -= mech;
                battery_power += mech * motors[a].efficiency;
            }
        }
        self.battery_energy_j = clamp(
            self.battery_energy_j - battery_power * dt,
            0.0,
            self.p.battery.capacity_j,
        );

        result.telemetry = DriveTelemetry {
            engine_rpm: self.motor_omega[0].abs() * RAD_S_TO_RPM,
            engine_torque_nm: torque[0] + torque[1],
            gear: self.gear,
            shifting: false,
            clutch_engagement: if self.gear != 0 { 1.0 } else { 0.0 },
            clutch_torque_nm: 0.0,
            motor_torque_nm: torque,
            drive_power_w: drive_power,
            regen_power_w: regen_power,
            battery_power_w: battery_power,
            battery_soc: self.battery_energy_j / self.p.battery.capacity_j,
            ..DriveTelemetry::default()
        };
        result
    }

    fn kinetic_energy(&self) -> f64 {
        0.5 * self.p.front_motor.rotor_inertia_kg_m2 * self.motor_omega[0] * self.motor_omega[0]
            + 0.5
                * self.p.rear_motor.rotor_inertia_kg_m2
                * self.motor_omega[1]
                * self.motor_omega[1]
    }

    fn reset(&mut self) {
        self.motor_omega = [0.0; 2];
        self.gear = 1;
        self.battery_energy_j = self.p.battery.capacity_j * self.p.battery.initial_soc;
    }

    fn forward_gears(&self) -> u8 {
        1
    }

    fn kind_name(&self) -> &'static str {
        "dual_motor_electric"
    }
}

impl Snapshot for DualMotor {
    fn save(&self, w: &mut StateWriter) {
        w.write_f64(self.motor_omega[0]);
        w.write_f64(self.motor_omega[1]);
        w.write_i8(self.gear);
        w.write_f64(self.battery_energy_j);
    }

    fn load(&mut self, r: &mut StateReader<'_>) -> Result<(), SnapshotError> {
        self.motor_omega[0] = r.read_f64()?;
        self.motor_omega[1] = r.read_f64()?;
        self.gear = r.read_i8()?;
        self.battery_energy_j = r.read_f64()?;
        if !(-1..=1).contains(&self.gear) {
            return Err(SnapshotError::Invalid("drive direction out of range"));
        }
        Ok(())
    }
}
