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

/// Axes an input device can report, in the order of [`AXIS_NAMES`].
pub const INPUT_AXES: usize = 8;
/// Buttons an input device can report.
pub const INPUT_BUTTONS: usize = 128;
/// Names of the axes, as DirectInput calls them.
pub const AXIS_NAMES: [&str; INPUT_AXES] =
    ["X", "Y", "Z", "Rx", "Ry", "Rz", "Slider 1", "Slider 2"];

/// Everything an input device reports at one moment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RawInputState {
    /// Axes, -1 to 1 across their full travel as the device reports them.
    /// An axis the device does not have reads 0.
    pub axes: [f32; INPUT_AXES],
    /// Buttons, one bit each, bit `n` for button `n`.
    pub buttons: u128,
}

impl RawInputState {
    /// True while button `index` is held.
    pub fn button(&self, index: usize) -> bool {
        index < INPUT_BUTTONS && self.buttons & (1u128 << index) != 0
    }
}

/// A device read for driver input: wheel, pedals, shifter, handbrake.
///
/// Opened without exclusive access, so the same device can be pushed by
/// an [`FfbDevice`] and read here at once.
pub trait InputSource: Send {
    /// What this device is.
    fn info(&self) -> &DeviceInfo;

    /// Reads the current state.
    fn read_state(&mut self) -> Result<RawInputState, FfbError>;

    /// Which of the axes the device actually has. An axis it lacks always
    /// reads 0.
    fn axes_present(&self) -> [bool; INPUT_AXES] {
        [true; INPUT_AXES]
    }

    /// How many buttons the device has.
    fn button_count(&self) -> usize {
        INPUT_BUTTONS
    }
}

/// An input device whose state is set by the caller, for tests.
#[derive(Clone, Debug)]
pub struct ScriptedInput {
    info: DeviceInfo,
    /// State the next read returns.
    pub state: RawInputState,
    /// Reads so far.
    pub reads: u64,
}

impl ScriptedInput {
    /// A scripted device with the given identifier.
    pub fn new(id: &str) -> Self {
        Self {
            info: DeviceInfo {
                id: id.to_string(),
                name: format!("Scripted {id}"),
                has_force_feedback: false,
                backend: "scripted",
            },
            state: RawInputState::default(),
            reads: 0,
        }
    }
}

impl InputSource for ScriptedInput {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    fn read_state(&mut self) -> Result<RawInputState, FfbError> {
        self.reads += 1;
        Ok(self.state)
    }
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
