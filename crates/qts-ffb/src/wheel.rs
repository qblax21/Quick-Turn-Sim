// SPDX-License-Identifier: Apache-2.0
//! Wheel-base models and force-feedback strength in real units.
//!
//! The pipeline works in newton-metres throughout: the simulation gives the
//! torque at the car's steering column, `gain` scales it to the torque
//! wanted at the driver's wheel, and the base's peak torque turns that into
//! a fraction of full output. So a gain means the same on every base:
//! gain 0.5 asks any wheel for half the car's column torque; only how much
//! of that a base can deliver before clipping differs. A [`WheelModel`] is
//! the hardware side: what the base can deliver.

use serde::Deserialize;

/// One wheel base, as in `data/wheels`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WheelModel {
    /// Identifier, equal to the data file stem.
    pub id: String,
    /// Display name.
    pub name: String,
    /// True when the peak torque was read from the manufacturer's own
    /// published figure; false when it could not be confirmed.
    pub verified: bool,
    /// What to know about the figure, shown next to it.
    #[serde(default)]
    pub note: String,
    /// Peak torque at the wheel rim, N·m, as the manufacturer states it.
    pub peak_torque_nm: f64,
}

impl WheelModel {
    /// Checks the figure is usable.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.peak_torque_nm.is_finite()
            && self.peak_torque_nm > 0.0
            && self.peak_torque_nm <= 60.0)
        {
            return Err(format!(
                "peak_torque_nm must be between 0 and 60, got {}",
                self.peak_torque_nm
            ));
        }
        Ok(())
    }
}

/// The gain at which a column torque of `column_nm` (for example the 99th
/// percentile over a hard lap) just reaches the soft-clip knee of a base
/// with `peak_nm` of peak torque, capped at 1 (the car's real torque).
pub fn suggest_gain(column_nm: f64, knee: f64, peak_nm: f64) -> f64 {
    if !(column_nm.is_finite() && column_nm > 0.0) {
        return 1.0;
    }
    (knee.clamp(0.0, 1.0) * peak_nm / column_nm).min(1.0)
}

/// Car steering input, −1 to 1, for the physical wheel at `device_fraction`
/// of its range (−1 to 1) on a base set to `device_rotation_rad` lock to
/// lock, matched one to one to a car whose steering wheel turns
/// `car_lock_rad` either way: a degree at the driver's wheel is a degree of
/// the car's. Past the car's lock the input stays at full lock; the soft
/// lock in the pipeline holds the wheel there.
pub fn matched_steer(device_fraction: f64, device_rotation_rad: f64, car_lock_rad: f64) -> f64 {
    if !(car_lock_rad > 0.0 && device_rotation_rad > 0.0) {
        return device_fraction.clamp(-1.0, 1.0);
    }
    (device_fraction * 0.5 * device_rotation_rad / car_lock_rad).clamp(-1.0, 1.0)
}

/// Torque at the driver's wheel, N·m, for a car column torque, before the
/// base's limit.
pub fn wheel_torque_nm(column_nm: f64, gain: f64) -> f64 {
    column_nm * gain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_gain_asks_every_base_for_the_same_torque() {
        // 10 N.m at the column with gain 0.4 is 4 N.m on any wheel: two
        // thirds of a 6 N.m base, a fifth of a 20 N.m base.
        let wheel = wheel_torque_nm(10.0, 0.4);
        assert!((wheel - 4.0).abs() < 1e-12);
        assert!((wheel / 6.0 - 2.0 / 3.0).abs() < 1e-12);
        assert!((wheel / 20.0 - 0.2).abs() < 1e-12);
    }

    #[test]
    fn matched_steering_turns_the_car_wheel_as_far_as_the_real_one() {
        let rot = 900_f64.to_radians();
        let lock = 378_f64.to_radians();
        // 90 degrees at a 900-degree base is 0.2 of its half range; in the
        // car that is 90 of 378 degrees.
        let s = matched_steer(90.0 / 450.0, rot, lock);
        assert!((s * lock - 90_f64.to_radians()).abs() < 1e-12);
        // Past the car's lock it stays at full lock.
        assert_eq!(matched_steer(0.9, rot, lock), 1.0);
        assert_eq!(matched_steer(-0.9, rot, lock), -1.0);
        // A base set to exactly the car's rotation maps end to end.
        assert!((matched_steer(1.0, 2.0 * lock, lock) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn suggested_gain_puts_the_hard_lap_at_the_knee() {
        let g = suggest_gain(12.0, 0.75, 6.0);
        assert!((g - 0.375).abs() < 1e-12);
        assert!((wheel_torque_nm(12.0, g) / 6.0 - 0.75).abs() < 1e-12);
        // A strong base runs the car's real torque, not more.
        assert_eq!(suggest_gain(12.0, 0.75, 25.0), 1.0);
        assert_eq!(suggest_gain(0.0, 0.75, 6.0), 1.0);
    }
}
