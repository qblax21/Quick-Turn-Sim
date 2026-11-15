// SPDX-License-Identifier: Apache-2.0
//! Input recording and exact replay.
//!
//! A replay stores the initial snapshot, the input frame for every tick and
//! a state hash every `hash_interval` ticks. Verifying a replay restores the
//! snapshot, re-simulates the frames and reports the first tick whose hash
//! differs.

use crate::input::InputFrame;
use crate::snapshot::{
    restore_bytes, snapshot_bytes, state_hash, SnapshotError, StateReader, StateWriter,
};
use crate::step::Simulation;
use std::fmt;

/// Magic bytes at the start of a replay file. Deliberately free of any
/// product name so a rename never breaks old files.
pub const REPLAY_MAGIC: [u8; 4] = *b"SRPL";

/// Version of the replay container layout.
pub const REPLAY_FORMAT_VERSION: u32 = 1;

/// Metadata stored at the start of a replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayHeader {
    /// Physics rate the recording was made at.
    pub tick_rate_hz: u32,
    /// Identifier of the car data file.
    pub car_id: String,
    /// Identifier of the track data file.
    pub track_id: String,
    /// Hash of all data files the session loaded.
    pub content_hash: u64,
    /// Build that made the recording, see [`crate::constants::build_id`].
    pub build_id: String,
    /// A state hash is stored after every this many ticks.
    pub hash_interval: u32,
    /// Tick number of the initial snapshot.
    pub start_tick: u64,
}

/// A complete recorded run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    /// Metadata.
    pub header: ReplayHeader,
    /// State at `header.start_tick`.
    pub initial_snapshot: Vec<u8>,
    /// One input frame per simulated tick.
    pub frames: Vec<InputFrame>,
    /// State hash after ticks `hash_interval`, `2 * hash_interval`, …
    /// counted from the start of the recording.
    pub hashes: Vec<u64>,
}

/// Error from decoding or verifying a replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayError {
    /// The file does not start with [`REPLAY_MAGIC`].
    BadMagic,
    /// The container version is not supported by this build.
    UnsupportedVersion(u32),
    /// The file is truncated or malformed.
    Corrupt(SnapshotError),
    /// The simulation is configured differently from the recording.
    Mismatch(String),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplayError::BadMagic => write!(f, "not a replay file"),
            ReplayError::UnsupportedVersion(v) => write!(f, "unsupported replay version {v}"),
            ReplayError::Corrupt(e) => write!(f, "replay is corrupt: {e}"),
            ReplayError::Mismatch(m) => write!(f, "replay does not match this session: {m}"),
        }
    }
}

impl std::error::Error for ReplayError {}

impl From<SnapshotError> for ReplayError {
    fn from(e: SnapshotError) -> Self {
        ReplayError::Corrupt(e)
    }
}

/// Outcome of re-simulating a replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayReport {
    /// Ticks re-simulated.
    pub ticks: u64,
    /// Hash checkpoints compared.
    pub checkpoints: usize,
    /// First recorded checkpoint that did not match, as
    /// `(tick offset from start, recorded hash, replayed hash)`.
    pub first_mismatch: Option<(u64, u64, u64)>,
    /// State hash after the final tick.
    pub final_hash: u64,
}

impl ReplayReport {
    /// True when every checkpoint matched.
    pub fn is_exact(&self) -> bool {
        self.first_mismatch.is_none()
    }
}

/// Builds a [`Replay`] while a simulation runs.
#[derive(Clone, Debug)]
pub struct ReplayRecorder {
    replay: Replay,
}

impl ReplayRecorder {
    /// Starts a recording from the current state of `sim`.
    pub fn start<S: Simulation>(mut header: ReplayHeader, sim: &S) -> Self {
        header.start_tick = sim.tick();
        header.tick_rate_hz = sim.tick_rate_hz();
        header.hash_interval = header.hash_interval.max(1);
        Self {
            replay: Replay {
                header,
                initial_snapshot: snapshot_bytes(sim),
                frames: Vec::new(),
                hashes: Vec::new(),
            },
        }
    }

    /// Records the frame that was just simulated. Call once per tick,
    /// immediately after `sim.step(&frame)`.
    pub fn record<S: Simulation>(&mut self, frame: InputFrame, sim: &S) {
        self.replay.frames.push(frame);
        let n = self.replay.frames.len() as u64;
        if n % u64::from(self.replay.header.hash_interval) == 0 {
            self.replay.hashes.push(state_hash(sim));
        }
    }

    /// Number of frames recorded so far.
    pub fn len(&self) -> usize {
        self.replay.frames.len()
    }

    /// True if nothing has been recorded.
    pub fn is_empty(&self) -> bool {
        self.replay.frames.is_empty()
    }

    /// Finishes the recording.
    pub fn finish(self) -> Replay {
        self.replay
    }
}

impl Replay {
    /// Encodes the replay to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = StateWriter::recording();
        w.write_raw(&REPLAY_MAGIC);
        w.write_u32(REPLAY_FORMAT_VERSION);
        w.write_u32(self.header.tick_rate_hz);
        w.write_str(&self.header.car_id);
        w.write_str(&self.header.track_id);
        w.write_u64(self.header.content_hash);
        w.write_str(&self.header.build_id);
        w.write_u32(self.header.hash_interval);
        w.write_u64(self.header.start_tick);
        w.write_bytes(&self.initial_snapshot);
        w.write_u64(self.frames.len() as u64);
        for f in &self.frames {
            f.write(&mut w);
        }
        w.write_u64(self.hashes.len() as u64);
        for &h in &self.hashes {
            w.write_u64(h);
        }
        w.into_bytes()
    }

    /// Decodes a replay from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ReplayError> {
        let mut r = StateReader::new(bytes);
        if r.read_raw(4).map_err(|_| ReplayError::BadMagic)? != REPLAY_MAGIC {
            return Err(ReplayError::BadMagic);
        }
        let version = r.read_u32()?;
        if version != REPLAY_FORMAT_VERSION {
            return Err(ReplayError::UnsupportedVersion(version));
        }
        let header = ReplayHeader {
            tick_rate_hz: r.read_u32()?,
            car_id: r.read_string()?,
            track_id: r.read_string()?,
            content_hash: r.read_u64()?,
            build_id: r.read_string()?,
            hash_interval: r.read_u32()?.max(1),
            start_tick: r.read_u64()?,
        };
        let initial_snapshot = r.read_bytes()?.to_vec();
        let frame_count = r.read_u64()? as usize;
        if frame_count > r.remaining() / InputFrame::ENCODED_LEN {
            return Err(ReplayError::Corrupt(SnapshotError::UnexpectedEnd));
        }
        let mut frames = Vec::with_capacity(frame_count);
        for _ in 0..frame_count {
            frames.push(InputFrame::read(&mut r)?);
        }
        let hash_count = r.read_u64()? as usize;
        if hash_count > r.remaining() / 8 {
            return Err(ReplayError::Corrupt(SnapshotError::UnexpectedEnd));
        }
        let mut hashes = Vec::with_capacity(hash_count);
        for _ in 0..hash_count {
            hashes.push(r.read_u64()?);
        }
        r.finish()?;
        Ok(Self {
            header,
            initial_snapshot,
            frames,
            hashes,
        })
    }

    /// Restores `sim` to the recorded initial state, re-simulates every
    /// frame and compares the recorded hashes.
    ///
    /// `sim` must already be built from the same car, track and data as the
    /// recording; the caller checks `header.content_hash` because only it
    /// knows how the data was loaded.
    pub fn verify<S: Simulation>(&self, sim: &mut S) -> Result<ReplayReport, ReplayError> {
        if sim.tick_rate_hz() != self.header.tick_rate_hz {
            return Err(ReplayError::Mismatch(format!(
                "tick rate is {} Hz, replay was recorded at {} Hz",
                sim.tick_rate_hz(),
                self.header.tick_rate_hz
            )));
        }
        restore_bytes(sim, &self.initial_snapshot)?;
        let interval = u64::from(self.header.hash_interval.max(1));
        let mut first_mismatch = None;
        let mut checkpoints = 0usize;
        for (i, frame) in self.frames.iter().enumerate() {
            sim.step(frame);
            let n = i as u64 + 1;
            if n % interval == 0 {
                let idx = (n / interval - 1) as usize;
                if let Some(&recorded) = self.hashes.get(idx) {
                    checkpoints += 1;
                    let now = state_hash(sim);
                    if now != recorded && first_mismatch.is_none() {
                        first_mismatch = Some((n, recorded, now));
                    }
                }
            }
        }
        Ok(ReplayReport {
            ticks: self.frames.len() as u64,
            checkpoints,
            first_mismatch,
            final_hash: state_hash(sim),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::Snapshot;

    /// A small nonlinear system standing in for the real simulation.
    #[derive(Clone, Debug, PartialEq)]
    struct Toy {
        tick: u64,
        x: f64,
        v: f64,
    }

    impl Snapshot for Toy {
        fn save(&self, w: &mut StateWriter) {
            w.write_u64(self.tick);
            w.write_f64(self.x);
            w.write_f64(self.v);
        }
        fn load(&mut self, r: &mut StateReader<'_>) -> Result<(), SnapshotError> {
            self.tick = r.read_u64()?;
            self.x = r.read_f64()?;
            self.v = r.read_f64()?;
            Ok(())
        }
    }

    impl Simulation for Toy {
        fn step(&mut self, input: &InputFrame) {
            let dt = self.tick_dt();
            let force = input.throttle_f64() * 10.0 - input.brake_f64() * 7.0
                + crate::math::fm::sin(self.x) * 3.0
                - 0.2 * self.v;
            self.v += force * dt;
            self.x += self.v * dt;
            self.tick += 1;
        }
        fn tick(&self) -> u64 {
            self.tick
        }
        fn tick_rate_hz(&self) -> u32 {
            500
        }
    }

    fn header() -> ReplayHeader {
        ReplayHeader {
            tick_rate_hz: 0,
            car_id: "car".into(),
            track_id: "track".into(),
            content_hash: 42,
            build_id: crate::constants::build_id(),
            hash_interval: 10,
            start_tick: 0,
        }
    }

    fn frame_for(i: u64) -> InputFrame {
        let t = i as f64 * 0.013;
        InputFrame::from_analog(t.sin(), (t * 0.7).cos().abs(), (t * 1.3).sin().abs(), 0.0, 0.0)
    }

    fn record(ticks: u64) -> (Replay, Toy) {
        let mut sim = Toy {
            tick: 5,
            x: 0.3,
            v: -1.0,
        };
        let mut rec = ReplayRecorder::start(header(), &sim);
        for i in 0..ticks {
            let f = frame_for(i);
            sim.step(&f);
            rec.record(f, &sim);
        }
        (rec.finish(), sim)
    }

    #[test]
    fn replay_reproduces_the_run_exactly() {
        let (replay, original) = record(1234);
        assert_eq!(replay.header.start_tick, 5);
        assert_eq!(replay.hashes.len(), 123);
        let mut sim = Toy {
            tick: 0,
            x: 0.0,
            v: 0.0,
        };
        let report = replay.verify(&mut sim).unwrap();
        assert!(report.is_exact());
        assert_eq!(report.checkpoints, 123);
        assert_eq!(sim, original);
    }

    #[test]
    fn bytes_round_trip() {
        let (replay, _) = record(77);
        let decoded = Replay::from_bytes(&replay.to_bytes()).unwrap();
        assert_eq!(decoded, replay);
    }

    #[test]
    fn a_changed_input_is_detected_at_the_next_checkpoint() {
        let (mut replay, _) = record(200);
        replay.frames[55].throttle ^= 0x0100;
        let mut sim = Toy {
            tick: 0,
            x: 0.0,
            v: 0.0,
        };
        let report = replay.verify(&mut sim).unwrap();
        assert_eq!(report.first_mismatch.map(|m| m.0), Some(60));
    }

    #[test]
    fn corrupt_files_are_rejected() {
        let (replay, _) = record(20);
        let bytes = replay.to_bytes();
        assert_eq!(Replay::from_bytes(b"nope"), Err(ReplayError::BadMagic));
        assert!(matches!(
            Replay::from_bytes(&bytes[..bytes.len() - 3]),
            Err(ReplayError::Corrupt(_))
        ));
        let mut wrong_version = bytes.clone();
        wrong_version[4] = 99;
        assert_eq!(
            Replay::from_bytes(&wrong_version),
            Err(ReplayError::UnsupportedVersion(99))
        );
    }
}
