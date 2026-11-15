// SPDX-License-Identifier: Apache-2.0
//! The fixed-step contract and the host-side driver that feeds it.

use crate::input::InputFrame;
use crate::snapshot::Snapshot;

/// A deterministic simulation advanced one tick at a time.
///
/// Implementations must make `step` a pure function of the current state,
/// the input frame and the tick number: no clocks, no randomness, no global
/// state, no unordered iteration.
pub trait Simulation: Snapshot {
    /// Advances the state by exactly one tick using `input`.
    fn step(&mut self, input: &InputFrame);

    /// Number of ticks simulated since the session began.
    fn tick(&self) -> u64;

    /// Physics rate in ticks per second.
    fn tick_rate_hz(&self) -> u32;

    /// Duration of one tick in seconds.
    fn tick_dt(&self) -> f64 {
        1.0 / f64::from(self.tick_rate_hz())
    }

    /// Simulated time in seconds, derived from the tick count.
    fn sim_time(&self) -> f64 {
        self.tick() as f64 * self.tick_dt()
    }
}

/// Converts elapsed real time into whole ticks plus a render interpolation
/// fraction.
///
/// This is host-side code. It decides *how many* ticks to run; it never
/// passes time into [`Simulation::step`]. Headless runs and tests do not use
/// it at all.
#[derive(Clone, Debug)]
pub struct FixedStepDriver {
    tick_dt: f64,
    accumulator: f64,
    max_ticks_per_advance: u32,
    dropped_time: f64,
}

impl FixedStepDriver {
    /// Creates a driver for the given physics rate. `max_ticks_per_advance`
    /// bounds catch-up after a stall so the host cannot spiral.
    pub fn new(tick_rate_hz: u32, max_ticks_per_advance: u32) -> Self {
        Self {
            tick_dt: 1.0 / f64::from(tick_rate_hz.max(1)),
            accumulator: 0.0,
            max_ticks_per_advance: max_ticks_per_advance.max(1),
            dropped_time: 0.0,
        }
    }

    /// Adds `elapsed_seconds` of real time and returns the number of ticks
    /// the host should now run.
    pub fn advance(&mut self, elapsed_seconds: f64) -> u32 {
        if elapsed_seconds.is_finite() && elapsed_seconds > 0.0 {
            self.accumulator += elapsed_seconds;
        }
        let due = (self.accumulator / self.tick_dt).floor();
        let ticks = if due > f64::from(self.max_ticks_per_advance) {
            // Too far behind: run the bounded number and drop the rest.
            let kept = f64::from(self.max_ticks_per_advance) * self.tick_dt;
            let frac = self.accumulator - due * self.tick_dt;
            self.dropped_time += self.accumulator - kept - frac;
            self.accumulator = frac;
            self.max_ticks_per_advance
        } else {
            self.accumulator -= due * self.tick_dt;
            due as u32
        };
        if self.accumulator < 0.0 {
            self.accumulator = 0.0;
        }
        ticks
    }

    /// Fraction of a tick accumulated but not yet simulated, in `[0, 1)`.
    /// The renderer blends the previous and current poses by this amount.
    pub fn alpha(&self) -> f64 {
        (self.accumulator / self.tick_dt).clamp(0.0, 1.0)
    }

    /// Total real time discarded because the host fell too far behind.
    pub fn dropped_time(&self) -> f64 {
        self.dropped_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_ticks_and_fraction_are_separated() {
        let mut d = FixedStepDriver::new(1000, 100);
        assert_eq!(d.advance(0.0105), 10);
        assert!((d.alpha() - 0.5).abs() < 1e-9);
        assert_eq!(d.advance(0.0005), 1);
        assert!(d.alpha() < 1e-9);
    }

    #[test]
    fn many_small_frames_add_up_without_drift() {
        let mut d = FixedStepDriver::new(1000, 100);
        let mut total = 0u32;
        for _ in 0..6000 {
            total += d.advance(1.0 / 60.0);
        }
        // 100 seconds of 60 Hz frames is 100 000 ticks, within one tick.
        assert!((i64::from(total) - 100_000).abs() <= 1);
    }

    #[test]
    fn catch_up_is_bounded_and_reported() {
        let mut d = FixedStepDriver::new(1000, 50);
        assert_eq!(d.advance(2.0), 50);
        assert!((d.dropped_time() - 1.95).abs() < 1e-9);
        assert!(d.alpha() < 1.0);
    }

    #[test]
    fn bad_elapsed_values_are_ignored() {
        let mut d = FixedStepDriver::new(1000, 50);
        assert_eq!(d.advance(f64::NAN), 0);
        assert_eq!(d.advance(-1.0), 0);
        assert_eq!(d.advance(f64::INFINITY), 0);
    }
}
