// SPDX-License-Identifier: Apache-2.0
//! Combustion engine, friction clutch, manual gearbox and one driven axle
//! with an open or limited-slip differential.

use super::{brake_share, DriveContext, DriveResult, DriveTelemetry, Drivetrain, RAD_S_TO_RPM};
use crate::params::{DifferentialKind, DrivenAxle, EngineManualParams};
use crate::spin::SpinSolver;
use qts_core::math::{clamp, interp_table};
use qts_core::snapshot::{Snapshot, SnapshotError, StateReader, StateWriter};

/// Gauss-Seidel sweeps per tick.
const SOLVER_ITERATIONS: u32 = 10;
/// Idle controller gain, N·m per rpm below idle.
const IDLE_GAIN_NM_PER_RPM: f64 = 0.6;
/// Rev limiter hysteresis, rpm.
const LIMITER_HYSTERESIS_RPM: f64 = 150.0;
/// Torque a locked differential (a spool) can carry between the wheels,
/// N·m: far beyond anything the tires can give.
const LOCKED_DIFF_NM: f64 = 1.0e6;
/// Pause between automatic shifts, s.
const AUTO_SHIFT_COOLDOWN_S: f64 = 0.4;
/// Speed below which reverse may be selected from a forward gear, m/s.
const REVERSE_SELECT_SPEED: f64 = 2.0;
/// A running engine dragged below this share of idle speed stalls.
const STALL_IDLE_FRACTION: f64 = 0.4;
/// A stalled engine on the starter fires once it turns faster than this
/// share of idle speed.
const FIRE_IDLE_FRACTION: f64 = 0.25;
/// A stalled engine turned by the wheels (a bump start) fires once it
/// turns faster than this share of idle speed. Above the stall speed, so a
/// dragged engine cannot flicker between stalled and running.
const BUMP_FIRE_IDLE_FRACTION: f64 = 0.6;
/// Starter motor torque at the crank at rest, N·m. It falls to nothing at
/// the cranking speed.
const STARTER_TORQUE_NM: f64 = 80.0;
/// Cranking speed as a share of idle speed.
const CRANK_IDLE_FRACTION: f64 = 0.35;
/// Clutch pedal travel at which the starter interlock switch closes. Real
/// cars put the switch close to the floor, below the release point.
const STARTER_INTERLOCK_TRAVEL: f64 = 0.85;
/// Damage per second at the failure speed, falling with the square of how
/// far past the damage speed the engine is. Placeholder: half a second at
/// the midpoint of the band costs an eighth of the engine.
const OVERREV_DAMAGE_RATE_PER_S: f64 = 1.0;
/// Share of full-throttle torque lost at full damage, before failure.
const DAMAGE_TORQUE_LOSS: f64 = 0.6;
/// Largest difference between engine speed and the speed the selected gear
/// would turn it at, rad/s, for a gear to go in without the clutch (a
/// rev-matched clutchless shift). About 150 rpm.
const SYNC_MATCH_RAD_S: f64 = 15.7;

/// The engine-and-manual-gearbox drivetrain module.
#[derive(Clone, Debug)]
pub struct EngineManual {
    p: EngineManualParams,
    torque_table: Vec<(f64, f64)>,
    driven: [usize; 2],
    engine_omega: f64,
    gear: i8,
    target_gear: i8,
    shift_timer: f64,
    limiter_cut: bool,
    diff_input_torque: f64,
    auto_shift_cooldown: f64,
    stalled: bool,
    grinding: bool,
    grind_speed: f64,
    peak_omega: f64,
    damage: f64,
}

impl EngineManual {
    /// Builds the module at idle in neutral.
    pub fn new(p: EngineManualParams) -> Self {
        let torque_table = p
            .engine
            .torque_curve_rpm
            .iter()
            .copied()
            .zip(p.engine.torque_curve_nm.iter().copied())
            .collect();
        let driven = match p.driven_axle {
            DrivenAxle::Front => [0, 1],
            DrivenAxle::Rear => [2, 3],
        };
        let idle = p.engine.idle_rpm / RAD_S_TO_RPM;
        Self {
            p,
            torque_table,
            driven,
            engine_omega: idle,
            gear: 0,
            target_gear: 0,
            shift_timer: 0.0,
            limiter_cut: false,
            diff_input_torque: 0.0,
            auto_shift_cooldown: 0.0,
            stalled: false,
            grinding: false,
            grind_speed: 0.0,
            peak_omega: idle,
            damage: 0.0,
        }
    }

    /// Engine damage, 0 (healthy) to 1 (failed).
    pub fn damage(&self) -> f64 {
        self.damage
    }

    /// True once over-revving has destroyed the engine.
    pub fn failed(&self) -> bool {
        self.damage >= 1.0
    }

    /// Engine speed, rad/s.
    pub fn engine_omega(&self) -> f64 {
        self.engine_omega
    }

    /// Overall ratio (gear times final drive) of `gear`; negative in
    /// reverse, zero in neutral.
    pub fn overall_ratio(&self, gear: i8) -> f64 {
        let g = &self.p.gearbox;
        match gear {
            0 => 0.0,
            n if n < 0 => -g.reverse_ratio * g.final_drive,
            n => match g.ratios.get(n as usize - 1) {
                Some(r) => r * g.final_drive,
                None => 0.0,
            },
        }
    }

    /// Full-throttle torque at an engine speed, N·m.
    pub fn full_throttle_torque(&self, rpm: f64) -> f64 {
        interp_table(&self.torque_table, rpm).max(0.0)
    }

    fn begin_shift(&mut self, gear: i8) {
        if gear == self.gear && self.shift_timer <= 0.0 {
            return;
        }
        self.target_gear = gear;
        if self.p.gearbox.shift_time_s > 0.0 {
            self.shift_timer = self.p.gearbox.shift_time_s;
        } else {
            self.gear = gear;
        }
        self.auto_shift_cooldown = AUTO_SHIFT_COOLDOWN_S;
    }

    fn handle_shifting(&mut self, ctx: &DriveContext, dt: f64) {
        let c = &ctx.controls;
        let top = self.p.gearbox.ratios.len() as i8;
        if self.auto_shift_cooldown > 0.0 {
            self.auto_shift_cooldown -= dt;
        }
        if self.shift_timer > 0.0 {
            self.shift_timer -= dt;
            if self.shift_timer <= 0.0 {
                self.shift_timer = 0.0;
                self.gear = self.target_gear;
            }
            return;
        }
        self.grinding = false;
        self.grind_speed = 0.0;
        if let Some(g) = c.gear_request {
            if (-1..=top).contains(&g) && g != self.gear {
                // An H-pattern lever: pulling out of gear always works, but
                // putting a gear in needs the clutch down or the engine
                // already turning at the speed that gear would turn it.
                let clutch_free = c.auto_clutch || self.p.clutch.engagement(c.clutch) <= 0.0;
                let [left, right] = self.driven;
                let shaft =
                    0.5 * (ctx.wheel_omega[left] + ctx.wheel_omega[right]) * self.overall_ratio(g);
                let matched = (self.engine_omega - shaft).abs() < SYNC_MATCH_RAD_S;
                if g == 0 {
                    // Out of gear is immediate: nothing has to synchronise.
                    self.gear = 0;
                    self.target_gear = 0;
                    self.shift_timer = 0.0;
                } else if clutch_free || matched {
                    self.begin_shift(g);
                } else {
                    // The synchroniser cannot bring the engine round: the
                    // lever stays at the gate, the box in neutral.
                    self.gear = 0;
                    self.target_gear = 0;
                    self.grinding = true;
                    self.grind_speed = (self.engine_omega - shaft).abs();
                }
                return;
            }
            // The lever says where the gearbox is; nothing else shifts it.
            return;
        }
        if c.shift_up && self.gear < top {
            self.begin_shift(self.gear + 1);
            return;
        }
        if c.shift_down && self.gear > -1 {
            let into_reverse = self.gear == 0;
            if !into_reverse || ctx.speed.abs() < REVERSE_SELECT_SPEED {
                self.begin_shift(self.gear - 1);
            }
            return;
        }
        if c.auto_shift && self.auto_shift_cooldown <= 0.0 {
            let rpm = self.engine_omega * RAD_S_TO_RPM;
            let redline = self.p.engine.redline_rpm;
            if self.gear == 0 && c.throttle > 0.05 {
                self.begin_shift(1);
            } else if self.gear >= 1 {
                if rpm > 0.97 * redline && self.gear < top && c.throttle > 0.3 {
                    self.begin_shift(self.gear + 1);
                } else if self.gear > 1 {
                    let here = self.overall_ratio(self.gear);
                    let lower = self.overall_ratio(self.gear - 1);
                    let rpm_after = rpm * lower / here;
                    if rpm < 0.4 * redline && rpm_after < 0.7 * redline {
                        self.begin_shift(self.gear - 1);
                    }
                }
            }
        }
    }
}

impl Drivetrain for EngineManual {
    fn step(&mut self, ctx: &DriveContext, dt: f64) -> DriveResult {
        self.handle_shifting(ctx, dt);
        let c = &ctx.controls;
        let e = &self.p.engine;
        let rpm = self.engine_omega * RAD_S_TO_RPM;

        // Clutch engagement.
        let in_gear = self.shift_timer <= 0.0 && self.gear != 0;
        let ratio = if in_gear {
            self.overall_ratio(self.gear)
        } else {
            0.0
        };
        // The pedal acts through free travel and a bite zone, not linearly.
        let pedal = self.p.clutch.engagement(clamp(c.clutch, 0.0, 1.0));
        let engagement = if !in_gear {
            0.0
        } else if c.auto_clutch {
            let cl = &self.p.clutch;
            let auto = clamp(
                (rpm - cl.auto_engage_start_rpm)
                    / (cl.auto_engage_full_rpm - cl.auto_engage_start_rpm),
                0.0,
                1.0,
            );
            pedal.min(auto)
        } else {
            pedal
        };

        // Stalling and starting. A stalled engine makes no torque of its
        // own; it fires again once turned fast enough.
        let idle_omega = e.idle_rpm / RAD_S_TO_RPM;
        // The starter needs the clutch down or the box in neutral; the
        // automatic clutch restarts a stalled engine by itself.
        let starter_engaged = !self.failed()
            && (c.auto_clutch
                || (c.starter && (c.clutch >= STARTER_INTERLOCK_TRAVEL || self.gear == 0)));
        if self.failed() {
            self.stalled = true;
        } else if self.stalled {
            let fire = if starter_engaged {
                FIRE_IDLE_FRACTION
            } else {
                BUMP_FIRE_IDLE_FRACTION
            };
            if self.engine_omega >= fire * idle_omega {
                self.stalled = false;
            }
        } else if self.engine_omega < STALL_IDLE_FRACTION * idle_omega && !starter_engaged {
            self.stalled = true;
        }
        // The starter turns the engine while held, until it runs on its own.
        let cranking = starter_engaged
            && (self.stalled || self.engine_omega < STALL_IDLE_FRACTION * idle_omega);
        let starter_torque = if cranking {
            STARTER_TORQUE_NM
                * (1.0 - self.engine_omega / (CRANK_IDLE_FRACTION * idle_omega)).max(0.0)
        } else {
            0.0
        };

        // Engine torque.
        if rpm > e.redline_rpm {
            self.limiter_cut = true;
        } else if rpm < e.redline_rpm - LIMITER_HYSTERESIS_RPM {
            self.limiter_cut = false;
        }
        let throttle = if self.limiter_cut {
            0.0
        } else {
            clamp(c.throttle, 0.0, 1.0)
        };
        let full = self.full_throttle_torque(rpm) * (1.0 - DAMAGE_TORQUE_LOSS * self.damage);
        // The torque curve is the published one, net of the engine's own
        // friction (what a dynamometer reads at the crank), so at full
        // throttle the engine gives the curve; friction is what it gives
        // with the throttle shut, the engine braking (D-093).
        let friction = if self.engine_omega > 0.0 {
            e.friction_torque_nm + e.friction_nm_per_rad_s * self.engine_omega
        } else {
            0.0
        };
        let mut engine_torque = if self.stalled {
            -friction
        } else {
            throttle * (full + friction) - friction
        };
        if rpm < e.idle_rpm && !self.stalled {
            let idle_limit = 0.6 * self.full_throttle_torque(e.idle_rpm);
            engine_torque += (IDLE_GAIN_NM_PER_RPM * (e.idle_rpm - rpm)).min(idle_limit);
        }
        engine_torque += starter_torque;

        // Coupled solve: four wheels plus the engine.
        const ENGINE: usize = 4;
        let omega = [
            ctx.wheel_omega[0],
            ctx.wheel_omega[1],
            ctx.wheel_omega[2],
            ctx.wheel_omega[3],
            self.engine_omega,
        ];
        let inertia = [
            ctx.wheel_inertia[0],
            ctx.wheel_inertia[1],
            ctx.wheel_inertia[2],
            ctx.wheel_inertia[3],
            e.inertia_kg_m2,
        ];
        let mut solver = SpinSolver::new(&omega, &inertia, dt);
        for i in 0..4 {
            solver.apply_torque(i, ctx.tire_torque[i]);
        }
        solver.apply_torque(ENGINE, engine_torque);

        let mut friction = [None; 4];
        let mut brake_cap = [0.0; 4];
        for i in 0..4 {
            brake_cap[i] = ctx.service_brake_torque[i] + ctx.handbrake_torque[i];
            friction[i] = solver.add_constraint(&[(i, 1.0)], brake_cap[i] + ctx.resist_torque[i]);
        }
        let [left, right] = self.driven;
        let clutch = if engagement > 0.0 {
            solver.add_constraint(
                &[(ENGINE, 1.0), (left, -0.5 * ratio), (right, -0.5 * ratio)],
                self.p.clutch.max_torque_nm * engagement,
            )
        } else {
            None
        };
        // Differential: the packs (or a spool) carry torque between the two
        // sides up to the locking torque, from last tick's input torque.
        let diff_lock = self.p.differential.locking_torque(self.diff_input_torque);
        let diff = if self.p.differential.kind == DifferentialKind::Open {
            None
        } else {
            solver.add_constraint(&[(left, 1.0), (right, -1.0)], diff_lock.min(LOCKED_DIFF_NM))
        };
        solver.solve(SOLVER_ITERATIONS);
        let diff_transfer = diff.map_or(0.0, |idx| solver.constraint_torque(idx));

        let mut result = DriveResult::default();
        for i in 0..4 {
            result.wheel_omega[i] = solver.omega(i);
            if let Some(idx) = friction[i] {
                result.brake_torque[i] = brake_share(
                    solver.constraint_torque(idx),
                    brake_cap[i],
                    ctx.resist_torque[i],
                );
            }
        }
        self.engine_omega = solver.omega(ENGINE).max(0.0);

        // Over-rev: the limiter only cuts fuel, so a wheel-driven engine
        // (a missed downshift) can be spun far past it.
        self.peak_omega = self.peak_omega.max(self.engine_omega);
        let rpm_now = self.engine_omega * RAD_S_TO_RPM;
        let overrev = rpm_now > e.overrev_damage_rpm;
        if c.engine_damage && overrev && !self.failed() {
            let x =
                (rpm_now - e.overrev_damage_rpm) / (e.overrev_failure_rpm - e.overrev_damage_rpm);
            self.damage = if x >= 1.0 {
                1.0
            } else {
                (self.damage + OVERREV_DAMAGE_RATE_PER_S * x * x * dt).min(1.0)
            };
        }

        // Positive clutch torque drives the gearbox.
        let clutch_torque = clutch.map_or(0.0, |idx| -solver.constraint_torque(idx));
        let axle_torque = clutch_torque * ratio;
        self.diff_input_torque = axle_torque * if ratio < 0.0 { -1.0 } else { 1.0 };
        result.shaft_torque[left] = 0.5 * axle_torque;
        result.shaft_torque[right] = 0.5 * axle_torque;

        let carrier = 0.5 * (result.wheel_omega[left] + result.wheel_omega[right]);
        let clutch_slip = if clutch.is_some() {
            self.engine_omega - carrier * ratio
        } else {
            0.0
        };
        result.telemetry = DriveTelemetry {
            engine_rpm: self.engine_omega * RAD_S_TO_RPM,
            engine_torque_nm: engine_torque,
            gear: if self.shift_timer > 0.0 {
                self.target_gear
            } else {
                self.gear
            },
            shifting: self.shift_timer > 0.0,
            clutch_engagement: engagement,
            clutch_torque_nm: clutch_torque,
            motor_torque_nm: [0.0; 2],
            drive_power_w: axle_torque * carrier,
            regen_power_w: 0.0,
            battery_power_w: 0.0,
            battery_soc: 0.0,
            stalled: self.stalled,
            grinding: self.grinding,
            grind_speed_rad_s: self.grind_speed,
            diff_lock_nm: if diff.is_some() {
                diff_lock.min(LOCKED_DIFF_NM)
            } else {
                0.0
            },
            diff_transfer_nm: diff_transfer,
            diff_input_nm: self.diff_input_torque,
            clutch_slip_rad_s: clutch_slip,
            clutch_capacity_nm: self.p.clutch.max_torque_nm,
            peak_rpm: self.peak_omega * RAD_S_TO_RPM,
            overrev,
            engine_damage: self.damage,
        };
        result
    }

    fn kinetic_energy(&self) -> f64 {
        0.5 * self.p.engine.inertia_kg_m2 * self.engine_omega * self.engine_omega
    }

    fn reset(&mut self) {
        *self = EngineManual::new(self.p.clone());
    }

    fn forward_gears(&self) -> u8 {
        self.p.gearbox.ratios.len() as u8
    }

    fn kind_name(&self) -> &'static str {
        "engine_manual"
    }
}

impl Snapshot for EngineManual {
    fn save(&self, w: &mut StateWriter) {
        w.write_f64(self.engine_omega);
        w.write_i8(self.gear);
        w.write_i8(self.target_gear);
        w.write_f64(self.shift_timer);
        w.write_bool(self.limiter_cut);
        w.write_f64(self.diff_input_torque);
        w.write_f64(self.auto_shift_cooldown);
        w.write_bool(self.stalled);
        w.write_bool(self.grinding);
        w.write_f64(self.grind_speed);
        w.write_f64(self.peak_omega);
        w.write_f64(self.damage);
    }

    fn load(&mut self, r: &mut StateReader<'_>) -> Result<(), SnapshotError> {
        self.engine_omega = r.read_f64()?;
        self.gear = r.read_i8()?;
        self.target_gear = r.read_i8()?;
        self.shift_timer = r.read_f64()?;
        self.limiter_cut = r.read_bool()?;
        self.diff_input_torque = r.read_f64()?;
        self.auto_shift_cooldown = r.read_f64()?;
        self.stalled = r.read_bool()?;
        self.grinding = r.read_bool()?;
        self.grind_speed = r.read_f64()?;
        self.peak_omega = r.read_f64()?;
        self.damage = r.read_f64()?;
        if !(0.0..=1.0).contains(&self.damage) {
            return Err(SnapshotError::Invalid("engine damage out of range"));
        }
        let top = self.p.gearbox.ratios.len() as i8;
        if !(-1..=top).contains(&self.gear) || !(-1..=top).contains(&self.target_gear) {
            return Err(SnapshotError::Invalid("gear out of range"));
        }
        Ok(())
    }
}
