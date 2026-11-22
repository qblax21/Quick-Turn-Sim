// SPDX-License-Identifier: Apache-2.0
//! The concrete simulation: sessions, tracks, data loading and hosts.
//!
//! - [`data`]: locating and loading files under `data/`, with provenance
//!   enforcement.
//! - [`track`]: surface map, paths, cones and timing gates.
//! - [`session`]: [`session::Session`], one car on one track, implementing
//!   [`qts_core::Simulation`].
//! - [`drivers`]: scripted drivers for headless runs and tests.
//! - [`telemetry`]: the channel list and the mapping from session state to
//!   telemetry rows.
//! - [`host`]: the real-time host that runs a session against the wall
//!   clock and drives force feedback. Hosts are the only place a clock is
//!   read.
//!
//! The `qts-sim` binary in this crate is a thin command-line wrapper.

pub mod data;
pub mod direct_input;
pub mod drivers;
pub mod ffb_log;
pub mod ghost;
pub mod host;
pub mod lap_telemetry;
pub mod playback;
pub mod session;
pub mod setup;
pub mod telemetry;
pub mod track;

pub use data::{DataError, DataRoot};
pub use session::{Session, SessionAssets, SessionConfig, Timing};
pub use setup::Setup;
pub use track::Track;
