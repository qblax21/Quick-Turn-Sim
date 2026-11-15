// SPDX-License-Identifier: Apache-2.0
//! Solver for the rotating parts of the car: wheels, engine, motors.
//!
//! Brakes, rolling resistance, a clutch and a limited-slip differential are
//! all friction elements: each tries to bring a relative speed to zero and
//! can transmit at most a given torque. Modelling them as steep viscous
//! friction is stiff. Instead each is a velocity constraint whose impulse
//! is clamped to `capacity × dt`, solved by a fixed number of
//! Gauss-Seidel sweeps (see `DECISIONS.md`, D-008). A constraint with
//! infinite capacity is a rigid gear mesh.
//!
//! The sweep count and constraint order are fixed, so the result is
//! reproducible.

/// Largest number of rotating bodies in one solve.
pub const MAX_BODIES: usize = 6;
/// Largest number of constraints in one solve.
pub const MAX_CONSTRAINTS: usize = 8;

#[derive(Clone, Copy, Debug)]
struct Constraint {
    jac: [f64; MAX_BODIES],
    limit: f64,
    impulse: f64,
    inv_eff_mass: f64,
}

/// One tick's worth of coupled rotational dynamics.
#[derive(Clone, Debug)]
pub struct SpinSolver {
    count: usize,
    inv_inertia: [f64; MAX_BODIES],
    omega: [f64; MAX_BODIES],
    constraints: [Constraint; MAX_CONSTRAINTS],
    constraint_count: usize,
    dt: f64,
}

impl SpinSolver {
    /// Starts a solve for bodies with the given speeds (rad/s) and
    /// inertias (kg·m²). At most [`MAX_BODIES`] bodies are used.
    pub fn new(omega: &[f64], inertia: &[f64], dt: f64) -> Self {
        let count = omega.len().min(inertia.len()).min(MAX_BODIES);
        let mut s = Self {
            count,
            inv_inertia: [0.0; MAX_BODIES],
            omega: [0.0; MAX_BODIES],
            constraints: [Constraint {
                jac: [0.0; MAX_BODIES],
                limit: 0.0,
                impulse: 0.0,
                inv_eff_mass: 0.0,
            }; MAX_CONSTRAINTS],
            constraint_count: 0,
            dt,
        };
        for i in 0..count {
            s.omega[i] = omega[i];
            s.inv_inertia[i] = if inertia[i] > 0.0 {
                1.0 / inertia[i]
            } else {
                0.0
            };
        }
        s
    }

    /// Applies a known torque to a body for the whole tick.
    pub fn apply_torque(&mut self, body: usize, torque: f64) {
        if body < self.count {
            self.omega[body] += torque * self.dt * self.inv_inertia[body];
        }
    }

    /// Adds a constraint that drives `Σ coeff · ω` to zero, transmitting at
    /// most `max_torque` (use `f64::INFINITY` for a rigid coupling).
    /// Returns its index, or `None` if the solver is full or the constraint
    /// involves no movable body.
    pub fn add_constraint(&mut self, terms: &[(usize, f64)], max_torque: f64) -> Option<usize> {
        if self.constraint_count >= MAX_CONSTRAINTS {
            return None;
        }
        let mut jac = [0.0; MAX_BODIES];
        for &(body, coeff) in terms {
            if body < self.count {
                jac[body] += coeff;
            }
        }
        let mut eff = 0.0;
        for i in 0..self.count {
            eff += jac[i] * jac[i] * self.inv_inertia[i];
        }
        if eff <= 0.0 {
            return None;
        }
        let idx = self.constraint_count;
        self.constraints[idx] = Constraint {
            jac,
            limit: max_torque.max(0.0) * self.dt,
            impulse: 0.0,
            inv_eff_mass: 1.0 / eff,
        };
        self.constraint_count += 1;
        Some(idx)
    }

    /// Runs `iterations` Gauss-Seidel sweeps over all constraints.
    pub fn solve(&mut self, iterations: u32) {
        for _ in 0..iterations {
            for c in 0..self.constraint_count {
                let con = self.constraints[c];
                let mut v = 0.0;
                for i in 0..self.count {
                    v += con.jac[i] * self.omega[i];
                }
                let wanted = con.impulse - v * con.inv_eff_mass;
                let clamped = wanted.clamp(-con.limit, con.limit);
                let delta = clamped - con.impulse;
                if delta != 0.0 {
                    for i in 0..self.count {
                        self.omega[i] += con.jac[i] * delta * self.inv_inertia[i];
                    }
                    self.constraints[c].impulse = clamped;
                }
            }
        }
    }

    /// Speed of a body after the solve, rad/s.
    pub fn omega(&self, body: usize) -> f64 {
        self.omega[body]
    }

    /// Average torque a constraint transmitted over the tick, N·m, in the
    /// direction of its positive coefficients.
    pub fn constraint_torque(&self, index: usize) -> f64 {
        if self.dt > 0.0 {
            self.constraints[index].impulse / self.dt
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 0.001;

    #[test]
    fn brake_stops_a_wheel_and_holds_it_without_reversing() {
        let mut omega = 5.0;
        for _ in 0..200 {
            let mut s = SpinSolver::new(&[omega], &[1.0], DT);
            s.apply_torque(0, 20.0); // drive torque below brake capacity
            s.add_constraint(&[(0, 1.0)], 500.0);
            s.solve(4);
            omega = s.omega(0);
            assert!(omega >= 0.0);
        }
        assert_eq!(omega, 0.0);
    }

    #[test]
    fn friction_below_demand_only_slows_the_body() {
        let mut s = SpinSolver::new(&[100.0], &[2.0], DT);
        let c = s.add_constraint(&[(0, 1.0)], 50.0).unwrap();
        s.solve(4);
        // 50 N·m on 2 kg·m² for 1 ms removes 0.025 rad/s.
        assert!((s.omega(0) - 99.975).abs() < 1e-12);
        assert!((s.constraint_torque(c) + 50.0).abs() < 1e-9);
    }

    #[test]
    fn locked_clutch_conserves_angular_momentum_through_an_open_diff() {
        // Engine at 300 rad/s, two wheels at 20 rad/s, overall ratio 10.
        let g = 10.0;
        let (je, jw) = (0.2, 1.5);
        let mut s = SpinSolver::new(&[20.0, 20.0, 300.0], &[jw, jw, je], DT);
        let c = s
            .add_constraint(&[(2, 1.0), (0, -g / 2.0), (1, -g / 2.0)], f64::INFINITY)
            .unwrap();
        s.solve(8);
        let carrier = 0.5 * (s.omega(0) + s.omega(1));
        assert!((s.omega(2) - g * carrier).abs() < 1e-9);
        // Both wheels started equal and an open differential sends them
        // equal torque, so they must still be equal.
        assert!((s.omega(0) - s.omega(1)).abs() < 1e-12);
        assert!(s.constraint_torque(c) < 0.0, "engine is slowed");
    }

    #[test]
    fn slipping_clutch_transmits_exactly_its_capacity() {
        let g = 8.0;
        let mut s = SpinSolver::new(&[0.0, 0.0, 400.0], &[1.5, 1.5, 0.2], DT);
        let c = s
            .add_constraint(&[(2, 1.0), (0, -g / 2.0), (1, -g / 2.0)], 150.0)
            .unwrap();
        s.solve(8);
        assert!((s.constraint_torque(c) + 150.0).abs() < 1e-9);
        // Each wheel receives capacity × ratio / 2.
        let wheel_torque = s.omega(0) * 1.5 / DT;
        assert!((wheel_torque - 150.0 * g / 2.0).abs() < 1e-6);
    }

    #[test]
    fn limited_slip_moves_torque_to_the_slower_wheel() {
        let mut s = SpinSolver::new(&[30.0, 20.0], &[1.0, 1.0], DT);
        let c = s.add_constraint(&[(0, 1.0), (1, -1.0)], 100.0).unwrap();
        s.solve(4);
        assert!(s.omega(0) < 30.0 && s.omega(1) > 20.0);
        assert!((s.omega(0) + s.omega(1) - 50.0).abs() < 1e-12);
        assert!((s.constraint_torque(c) + 100.0).abs() < 1e-9);
    }

    #[test]
    fn full_solver_and_degenerate_constraints_are_rejected() {
        let mut s = SpinSolver::new(&[1.0], &[1.0], DT);
        assert!(s.add_constraint(&[(3, 1.0)], 10.0).is_none());
        for _ in 0..MAX_CONSTRAINTS {
            assert!(s.add_constraint(&[(0, 1.0)], 1.0).is_some());
        }
        assert!(s.add_constraint(&[(0, 1.0)], 1.0).is_none());
    }
}
