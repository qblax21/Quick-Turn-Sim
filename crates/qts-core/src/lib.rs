// SPDX-License-Identifier: Apache-2.0
//! Deterministic simulation mechanism.
//!
//! This crate is the bottom layer of the workspace. It defines *how* a
//! simulation is stepped, saved, hashed and replayed, and provides the math
//! every other crate uses. It knows nothing about cars or tires.
//!
//! - [`math`]: vectors, quaternions, a small linear solver, stateless noise,
//!   and the [`math::fm`] switch point for transcendental functions.
//! - [`snapshot`]: the [`snapshot::Snapshot`] trait, byte writer and reader,
//!   and the state hash.
//! - [`input`]: fixed-point [`input::InputFrame`].
//! - [`step`]: the [`step::Simulation`] contract and the host-side
//!   [`step::FixedStepDriver`].
//! - [`replay`]: input recording and exact replay.
//! - [`net`]: interfaces a future network layer plugs into.
//! - [`provenance`]: source and confidence of data-file parameters.
//! - [`constants`]: the product name and shared constants.

pub mod constants;
pub mod input;
pub mod math;
pub mod net;
pub mod provenance;
pub mod replay;
pub mod snapshot;
pub mod step;

pub use input::InputFrame;
pub use math::{Quat, Vec3};
pub use snapshot::{Snapshot, SnapshotError, StateReader, StateWriter};
pub use step::Simulation;
