// SPDX-License-Identifier: Apache-2.0
//! Platform backends for steering devices.
//!
//! Everything platform-specific in the workspace is under this module. The
//! functions here pick the backend for the current target; callers never
//! name a backend type.
//!
//! - Windows: DirectInput ([`dinput`]).
//! - Other platforms: no backend yet. [`enumerate`] returns an empty list
//!   and [`open`] reports that the platform is unsupported, so everything
//!   above this module still builds and runs with the null device. A Linux
//!   backend (evdev force feedback) would be a new sibling module wired in
//!   here.

use crate::device::{DeviceInfo, FfbDevice, FfbError, InputSource};

#[cfg(windows)]
pub mod dinput;

/// Lists attached steering devices.
pub fn enumerate() -> Result<Vec<DeviceInfo>, FfbError> {
    #[cfg(windows)]
    {
        dinput::enumerate()
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

/// Opens a device for reading its axes and buttons, without exclusive
/// access, by the identifier [`enumerate`] returned.
pub fn open_input(id: &str) -> Result<Box<dyn InputSource>, FfbError> {
    #[cfg(windows)]
    {
        Ok(Box::new(dinput::DInputReader::open(id)?))
    }
    #[cfg(not(windows))]
    {
        let _ = id;
        Err(FfbError::Unsupported(
            "direct device input is only implemented for Windows (DirectInput) so far",
        ))
    }
}

/// Opens a device by the identifier [`enumerate`] returned.
///
/// `window` is the native handle of the application's main window where the
/// platform needs one (Windows does, for exclusive device access).
pub fn open(id: &str, window: Option<isize>) -> Result<Box<dyn FfbDevice>, FfbError> {
    #[cfg(windows)]
    {
        Ok(Box::new(dinput::DInputDevice::open(id, window)?))
    }
    #[cfg(not(windows))]
    {
        let _ = (id, window);
        Err(FfbError::Unsupported(
            "force-feedback output is only implemented for Windows (DirectInput) so far",
        ))
    }
}
