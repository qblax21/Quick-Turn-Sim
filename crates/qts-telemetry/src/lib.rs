// SPDX-License-Identifier: Apache-2.0
//! Telemetry logging and data-logger import.
//!
//! - [`writer`]: the documented CSV format the simulator writes, and a
//!   reader for it. The format is specified in `docs/TELEMETRY_FORMAT.md`.
//! - [`import`]: reading CSV exports from data loggers through a
//!   column-mapping profile, and summarising a run into the quantities the
//!   validation harness compares against. The guide is
//!   `docs/TELEMETRY_IMPORT.md`.
//!
//! This crate knows nothing about cars: channels are named columns of
//! numbers. The simulator decides which channels exist.

pub mod import;
pub mod writer;

pub use writer::{read_telemetry, Channel, TelemetryFile, TelemetryWriter};
