// SPDX-License-Identifier: Apache-2.0
//! Networking hooks: types and traits only.
//!
//! Nothing here opens a socket. These are the seams a future
//! server-authoritative implementation plugs into: clients send
//! [`StampedInput`], the server sends [`SnapshotEnvelope`], and a
//! [`TickClock`] keeps a client's tick aligned with the server's. Netcode,
//! matchmaking, ranking and anti-cheat are out of scope for the foundation.

use crate::input::InputFrame;
use crate::snapshot::{fnv1a64, snapshot_bytes};
use crate::step::Simulation;

/// An input frame tagged with the tick it applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StampedInput {
    /// Tick this input is for.
    pub tick: u64,
    /// The input.
    pub frame: InputFrame,
}

/// An authoritative state snapshot as it would travel from a server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotEnvelope {
    /// Tick the snapshot was taken after.
    pub tick: u64,
    /// Hash of `bytes`, for cheap comparison before applying.
    pub hash: u64,
    /// Snapshot bytes, as produced by [`crate::snapshot::snapshot_bytes`].
    pub bytes: Vec<u8>,
}

impl SnapshotEnvelope {
    /// Captures the current state of `sim`.
    pub fn capture<S: Simulation>(sim: &S) -> Self {
        let bytes = snapshot_bytes(sim);
        Self {
            tick: sim.tick(),
            hash: fnv1a64(&bytes),
            bytes,
        }
    }
}

/// Where a session gets the input for a given tick: a local device, a
/// replay, or later a network jitter buffer.
pub trait InputSource {
    /// Returns the input for `tick`, or `None` if it is not available yet.
    fn input_for(&mut self, tick: u64) -> Option<InputFrame>;
}

/// Where authoritative snapshots are delivered on a client.
pub trait SnapshotSink {
    /// Called when a snapshot arrives. A rollback implementation would
    /// restore it and re-simulate local inputs up to the present tick.
    fn deliver(&mut self, snapshot: SnapshotEnvelope);
}

/// Maps a remote tick estimate onto the local timeline.
pub trait TickClock {
    /// Records that the remote side was at `remote_tick` when the local side
    /// was at `local_tick`.
    fn observe(&mut self, local_tick: u64, remote_tick: u64);

    /// Tick the local simulation should be at to stay aligned, given the
    /// current local tick.
    fn target_tick(&self, local_tick: u64) -> u64;
}

/// An [`InputSource`] backed by a fixed list of frames. Used by replays and
/// tests; frames past the end repeat the last one.
#[derive(Clone, Debug, Default)]
pub struct ScriptedInput {
    start_tick: u64,
    frames: Vec<InputFrame>,
}

impl ScriptedInput {
    /// Creates a source whose first frame applies to `start_tick`.
    pub fn new(start_tick: u64, frames: Vec<InputFrame>) -> Self {
        Self { start_tick, frames }
    }
}

impl InputSource for ScriptedInput {
    fn input_for(&mut self, tick: u64) -> Option<InputFrame> {
        if tick < self.start_tick || self.frames.is_empty() {
            return None;
        }
        let idx = ((tick - self.start_tick) as usize).min(self.frames.len() - 1);
        Some(self.frames[idx])
    }
}

/// A [`TickClock`] for a single local machine: the target is always the
/// local tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalClock;

impl TickClock for LocalClock {
    fn observe(&mut self, _local_tick: u64, _remote_tick: u64) {}
    fn target_tick(&self, local_tick: u64) -> u64 {
        local_tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_input_holds_last_frame() {
        let a = InputFrame::from_analog(0.1, 0.0, 0.0, 0.0, 0.0);
        let b = InputFrame::from_analog(0.2, 0.0, 0.0, 0.0, 0.0);
        let mut s = ScriptedInput::new(10, vec![a, b]);
        assert_eq!(s.input_for(9), None);
        assert_eq!(s.input_for(10), Some(a));
        assert_eq!(s.input_for(11), Some(b));
        assert_eq!(s.input_for(500), Some(b));
    }

    #[test]
    fn local_clock_targets_local_tick() {
        let mut c = LocalClock;
        c.observe(5, 900);
        assert_eq!(c.target_tick(7), 7);
    }
}
