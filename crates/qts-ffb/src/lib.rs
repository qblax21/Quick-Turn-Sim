// SPDX-License-Identifier: Apache-2.0
//! Force feedback.
//!
//! The simulation outputs raw physical steering torque. This crate turns
//! that into a device command and sends it:
//!
//! - [`pipeline`]: the configurable torque pipeline (gain, low-pass, slew
//!   limit, damping, friction, inertia, detail channels, soft clip). Pure
//!   computation, no platform code, fully unit tested.
//! - [`device`]: the [`device::FfbDevice`] trait and a null device.
//! - [`platform`]: device backends. All platform-specific code in the
//!   workspace is in there.
//!
//! The pipeline is deliberately outside the deterministic simulation state:
//! its damping, friction and inertia terms depend on how the physical wheel
//! moves, which is real-world input.

pub mod device;
pub mod pipeline;
pub mod platform;

pub use device::{DeviceInfo, FfbDevice, FfbError, NullDevice};
pub use pipeline::{soft_clip, FfbConfig, FfbInput, FfbPipeline, FfbSample};
