// SPDX-License-Identifier: Apache-2.0
//! Driver input for one tick, in fixed-point form.
//!
//! Hosts quantise device values into an [`InputFrame`]; the simulation only
//! ever sees the integers. A replay file or a network packet therefore
//! carries exactly what was simulated (see `DECISIONS.md`, D-005).

use crate::snapshot::{SnapshotError, StateReader, StateWriter};

/// Full-scale value for analog axes.
pub const AXIS_MAX: i16 = i16::MAX;

/// `gear_request` value meaning "no direct gear selection this tick".
pub const GEAR_NO_REQUEST: i8 = i8::MIN;

/// Bit flags for [`InputFrame::buttons`].
pub mod buttons {
    /// Sequential upshift, acted on at the rising edge.
    pub const SHIFT_UP: u16 = 1 << 0;
    /// Sequential downshift, acted on at the rising edge.
    pub const SHIFT_DOWN: u16 = 1 << 1;
    /// Return the car to the track's spawn point, at the rising edge.
    pub const RESET: u16 = 1 << 2;
    /// Starter motor, while held.
    pub const STARTER: u16 = 1 << 3;
}

/// Complete driver input for one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputFrame {
    /// Steering, `-AXIS_MAX` (full right) to `AXIS_MAX` (full left).
    pub steer: i16,
    /// Accelerator, `0` to `AXIS_MAX`.
    pub throttle: i16,
    /// Brake pedal, `0` to `AXIS_MAX`.
    pub brake: i16,
    /// Clutch pedal, `0` (engaged) to `AXIS_MAX` (fully pressed).
    pub clutch: i16,
    /// Handbrake, `0` to `AXIS_MAX`.
    pub handbrake: i16,
    /// Direct gear selection: `-1` reverse, `0` neutral, `1..` forward gears,
    /// or [`GEAR_NO_REQUEST`].
    pub gear_request: i8,
    /// Button bit field, see [`buttons`].
    pub buttons: u16,
}

impl Default for InputFrame {
    fn default() -> Self {
        Self {
            steer: 0,
            throttle: 0,
            brake: 0,
            clutch: 0,
            handbrake: 0,
            gear_request: GEAR_NO_REQUEST,
            buttons: 0,
        }
    }
}

fn quantize_signed(v: f64) -> i16 {
    let v = if v.is_finite() { v } else { 0.0 };
    (v.clamp(-1.0, 1.0) * f64::from(AXIS_MAX)).round() as i16
}

fn quantize_unsigned(v: f64) -> i16 {
    let v = if v.is_finite() { v } else { 0.0 };
    (v.clamp(0.0, 1.0) * f64::from(AXIS_MAX)).round() as i16
}

impl InputFrame {
    /// Size of the encoded frame in bytes.
    pub const ENCODED_LEN: usize = 13;

    /// Builds a frame from analog values. Steering is in `[-1, 1]` with
    /// positive to the left; the pedals are in `[0, 1]`. Out-of-range and
    /// non-finite values are clamped.
    pub fn from_analog(steer: f64, throttle: f64, brake: f64, clutch: f64, handbrake: f64) -> Self {
        Self {
            steer: quantize_signed(steer),
            throttle: quantize_unsigned(throttle),
            brake: quantize_unsigned(brake),
            clutch: quantize_unsigned(clutch),
            handbrake: quantize_unsigned(handbrake),
            gear_request: GEAR_NO_REQUEST,
            buttons: 0,
        }
    }

    /// Returns the frame with the given button bits set.
    pub fn with_buttons(mut self, bits: u16) -> Self {
        self.buttons |= bits;
        self
    }

    /// Returns the frame with a direct gear selection.
    pub fn with_gear(mut self, gear: i8) -> Self {
        self.gear_request = gear;
        self
    }

    /// Steering in `[-1, 1]`, positive to the left.
    pub fn steer_f64(&self) -> f64 {
        (f64::from(self.steer) / f64::from(AXIS_MAX)).clamp(-1.0, 1.0)
    }
    /// Accelerator in `[0, 1]`.
    pub fn throttle_f64(&self) -> f64 {
        (f64::from(self.throttle) / f64::from(AXIS_MAX)).clamp(0.0, 1.0)
    }
    /// Brake pedal in `[0, 1]`.
    pub fn brake_f64(&self) -> f64 {
        (f64::from(self.brake) / f64::from(AXIS_MAX)).clamp(0.0, 1.0)
    }
    /// Clutch pedal in `[0, 1]`, 1 is fully pressed.
    pub fn clutch_f64(&self) -> f64 {
        (f64::from(self.clutch) / f64::from(AXIS_MAX)).clamp(0.0, 1.0)
    }
    /// Handbrake in `[0, 1]`.
    pub fn handbrake_f64(&self) -> f64 {
        (f64::from(self.handbrake) / f64::from(AXIS_MAX)).clamp(0.0, 1.0)
    }
    /// True if the given button bit is set.
    pub fn pressed(&self, bit: u16) -> bool {
        self.buttons & bit != 0
    }

    /// Writes the frame in its fixed 13-byte encoding.
    pub fn write(&self, w: &mut StateWriter) {
        w.write_i16(self.steer);
        w.write_i16(self.throttle);
        w.write_i16(self.brake);
        w.write_i16(self.clutch);
        w.write_i16(self.handbrake);
        w.write_i8(self.gear_request);
        w.write_u16(self.buttons);
    }

    /// Reads a frame written by [`InputFrame::write`].
    pub fn read(r: &mut StateReader<'_>) -> Result<Self, SnapshotError> {
        Ok(Self {
            steer: r.read_i16()?,
            throttle: r.read_i16()?,
            brake: r.read_i16()?,
            clutch: r.read_i16()?,
            handbrake: r.read_i16()?,
            gear_request: r.read_i8()?,
            buttons: r.read_u16()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analog_values_are_clamped_and_round_trip() {
        let f = InputFrame::from_analog(2.0, -1.0, 0.5, f64::NAN, 1.0);
        assert_eq!(f.steer, AXIS_MAX);
        assert_eq!(f.throttle, 0);
        assert_eq!(f.clutch, 0);
        assert_eq!(f.handbrake, AXIS_MAX);
        assert!((f.brake_f64() - 0.5).abs() < 1e-4);
        assert_eq!(f.steer_f64(), 1.0);
    }

    #[test]
    fn encoding_is_fixed_length_and_lossless() {
        let f = InputFrame::from_analog(-0.25, 0.9, 0.1, 0.0, 0.0)
            .with_buttons(buttons::SHIFT_UP | buttons::RESET)
            .with_gear(3);
        let mut w = StateWriter::recording();
        f.write(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(bytes.len(), InputFrame::ENCODED_LEN);
        let mut r = StateReader::new(&bytes);
        assert_eq!(InputFrame::read(&mut r).unwrap(), f);
    }
}
