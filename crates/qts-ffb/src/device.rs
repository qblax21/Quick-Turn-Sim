// SPDX-License-Identifier: Apache-2.0
//! The device trait every platform backend implements.

use std::fmt;

/// Description of a steering device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable identifier used to open the device again.
    pub id: String,
    /// Name reported by the device or driver.
    pub name: String,
    /// True if the device accepts force commands.
    pub has_force_feedback: bool,
    /// Name of the backend that found it.
    pub backend: &'static str,
}

/// Error from a force-feedback device or backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FfbError {
    /// This build has no backend for the current platform.
    Unsupported(&'static str),
    /// No device with the requested identifier is attached.
    NotFound(String),
    /// The device refused an operation.
    Device(String),
}

impl fmt::Display for FfbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FfbError::Unsupported(what) => write!(f, "not supported on this platform: {what}"),
            FfbError::NotFound(id) => write!(f, "device `{id}` not found"),
            FfbError::Device(message) => write!(f, "device error: {message}"),
        }
    }
}

impl std::error::Error for FfbError {}

/// A steering device that can be pushed.
///
/// Implementations live in [`crate::platform`]. Everything outside that
/// module talks to devices only through this trait, so adding a platform
/// means adding one backend module.
pub trait FfbDevice: Send {
    /// What this device is.
    fn info(&self) -> &DeviceInfo;

    /// Commands a torque as a fraction of the device's peak, −1 to 1.
    /// Values outside the range are clamped. The sign is the device's own;
    /// the pipeline's `invert` setting adapts to it.
    fn set_torque(&mut self, normalized: f32) -> Result<(), FfbError>;

    /// Reads the steering axis as the device reports it, −1 to 1 across
    /// its full rotation with positive to the right, or `None` if the
    /// device cannot report position.
    fn read_steering(&mut self) -> Result<Option<f32>, FfbError>;

    /// Removes all force. Called when a session pauses or ends.
    fn stop(&mut self) -> Result<(), FfbError>;
}

/// A device that goes nowhere. It records what it was told, for tests and
/// for running without a wheel attached.
#[derive(Clone, Debug)]
pub struct NullDevice {
    info: DeviceInfo,
    /// Last torque command received.
    pub last_torque: f32,
    /// Number of torque commands received.
    pub commands: u64,
    /// Number of times [`FfbDevice::stop`] was called.
    pub stops: u64,
    /// Steering position this device will report.
    pub steering: Option<f32>,
}

impl Default for NullDevice {
    fn default() -> Self {
        Self {
            info: DeviceInfo {
                id: "null".into(),
                name: "No device".into(),
                has_force_feedback: false,
                backend: "null",
            },
            last_torque: 0.0,
            commands: 0,
            stops: 0,
            steering: None,
        }
    }
}

impl FfbDevice for NullDevice {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    fn set_torque(&mut self, normalized: f32) -> Result<(), FfbError> {
        self.last_torque = if normalized.is_finite() {
            normalized.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        self.commands += 1;
        Ok(())
    }

    fn read_steering(&mut self) -> Result<Option<f32>, FfbError> {
        Ok(self.steering)
    }

    fn stop(&mut self) -> Result<(), FfbError> {
        self.last_torque = 0.0;
        self.stops += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_device_clamps_and_counts() {
        let mut d = NullDevice::default();
        d.set_torque(3.0).unwrap();
        assert_eq!(d.last_torque, 1.0);
        d.set_torque(f32::NAN).unwrap();
        assert_eq!(d.last_torque, 0.0);
        assert_eq!(d.commands, 2);
        assert_eq!(d.read_steering().unwrap(), None);
        d.stop().unwrap();
        assert_eq!(d.stops, 1);
        assert!(!d.info().has_force_feedback);
    }
}
