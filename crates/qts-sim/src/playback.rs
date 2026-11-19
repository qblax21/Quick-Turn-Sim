// SPDX-License-Identifier: Apache-2.0
//! Watching a recorded replay.
//!
//! A replay holds the starting state and every input frame, so playback
//! re-simulates it. The re-simulation is done once, a slice at a time so a
//! front end stays responsive, and keeps a pose and a few display values
//! every [`PLAYBACK_SAMPLE_TICKS`] ticks. Watching, pausing and scrubbing
//! then only read those samples: going backwards costs nothing and nothing
//! is simulated twice. The stored state hashes are checked on the way, so
//! a replay that no longer reproduces (another build, changed code) is
//! reported rather than silently shown differently.

use crate::data::DataRoot;
use crate::host::RenderPose;
use crate::session::Session;
use qts_core::replay::Replay;
use qts_core::snapshot::{restore_bytes, state_hash};
use qts_core::step::Simulation;

/// Ticks between stored samples: 100 a second at the default rate.
pub const PLAYBACK_SAMPLE_TICKS: u64 = 10;

/// What is shown for one sample.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlaybackSample {
    /// Pose of the car.
    pub pose: RenderPose,
    /// Speed over the ground, m/s.
    pub speed: f64,
    /// Selected gear.
    pub gear: i8,
    /// Engine or front-motor speed, rpm.
    pub engine_rpm: f64,
    /// Steering-wheel angle, rad.
    pub steering_wheel_angle: f64,
    /// Driver inputs, 0 to 1 (steer -1 to 1).
    pub steer: f64,
    /// Accelerator.
    pub throttle: f64,
    /// Brake.
    pub brake: f64,
    /// Time into the current timed run, s; negative outside a run.
    pub run_time_s: f64,
}

/// A replay being prepared and then watched.
pub struct Playback {
    replay: Replay,
    session: Session,
    next_frame: usize,
    samples: Vec<PlaybackSample>,
    /// Cone states, recorded when they change: (sample index, states).
    cones: Vec<(usize, Vec<bool>)>,
    first_mismatch: Option<u64>,
}

impl Playback {
    /// Loads a replay and builds the session it was recorded in. Fails if
    /// the car, track or data files are missing or have changed.
    pub fn load(root: &DataRoot, bytes: &[u8]) -> Result<Self, String> {
        let replay = Replay::from_bytes(bytes).map_err(|e| e.to_string())?;
        let h = &replay.header;
        let mut session = Session::for_replay(root, h)?;
        restore_bytes(&mut session, &replay.initial_snapshot).map_err(|e| e.to_string())?;
        let mut playback = Self {
            replay,
            session,
            next_frame: 0,
            samples: Vec::new(),
            cones: Vec::new(),
            first_mismatch: None,
        };
        playback.take_sample(None);
        Ok(playback)
    }

    /// The session being replayed, for the track and car it uses.
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Re-simulates up to `max_ticks` more frames. Returns the share of the
    /// replay prepared so far, 0 to 1.
    pub fn prepare(&mut self, max_ticks: usize) -> f64 {
        let interval = u64::from(self.replay.header.hash_interval.max(1));
        let end = (self.next_frame + max_ticks).min(self.replay.frames.len());
        while self.next_frame < end {
            let frame = self.replay.frames[self.next_frame];
            self.session.step(&frame);
            self.next_frame += 1;
            let n = self.next_frame as u64;
            if n % interval == 0 && self.first_mismatch.is_none() {
                if let Some(&recorded) = self.replay.hashes.get((n / interval - 1) as usize) {
                    if state_hash(&self.session) != recorded {
                        self.first_mismatch = Some(n);
                    }
                }
            }
            if n % PLAYBACK_SAMPLE_TICKS == 0 {
                self.take_sample(Some(frame));
            }
        }
        self.progress()
    }

    /// Share of the replay prepared, 0 to 1.
    pub fn progress(&self) -> f64 {
        if self.replay.frames.is_empty() {
            1.0
        } else {
            self.next_frame as f64 / self.replay.frames.len() as f64
        }
    }

    /// True once the whole replay has been re-simulated.
    pub fn is_prepared(&self) -> bool {
        self.next_frame >= self.replay.frames.len()
    }

    /// Length of the replay, s.
    pub fn duration_s(&self) -> f64 {
        self.replay.frames.len() as f64 / f64::from(self.replay.header.tick_rate_hz.max(1))
    }

    /// Length of the part prepared so far, s.
    pub fn prepared_s(&self) -> f64 {
        self.samples.len().saturating_sub(1) as f64 * self.sample_dt()
    }

    /// The tick, counted from the start of the replay, at which the
    /// re-simulation first stopped matching the recording, if it did.
    pub fn first_mismatch(&self) -> Option<u64> {
        self.first_mismatch
    }

    /// The build that recorded the replay.
    pub fn recorded_by(&self) -> &str {
        &self.replay.header.build_id
    }

    fn sample_dt(&self) -> f64 {
        PLAYBACK_SAMPLE_TICKS as f64 / f64::from(self.replay.header.tick_rate_hz.max(1))
    }

    fn take_sample(&mut self, frame: Option<qts_core::InputFrame>) {
        let s = &self.session;
        let out = s.car().output();
        let timing = s.timing();
        let frame = frame.unwrap_or_default();
        self.samples.push(PlaybackSample {
            pose: RenderPose::of(s),
            speed: out.speed,
            gear: out.drive.gear,
            engine_rpm: out.drive.engine_rpm,
            steering_wheel_angle: out.steering_wheel_angle,
            steer: frame.steer_f64(),
            throttle: frame.throttle_f64(),
            brake: frame.brake_f64(),
            run_time_s: if timing.running { s.run_time() } else { -1.0 },
        });
        let index = self.samples.len() - 1;
        if self
            .cones
            .last()
            .is_none_or(|(_, c)| *c != timing.cone_down)
        {
            self.cones.push((index, timing.cone_down.clone()));
        }
    }

    /// The car at `time_s` into the replay, between the two nearest samples.
    /// Times beyond the prepared part show its last sample.
    pub fn at(&self, time_s: f64) -> PlaybackSample {
        let position = (time_s / self.sample_dt()).max(0.0);
        let last = self.samples.len() - 1;
        let index = (position.floor() as usize).min(last);
        let next = (index + 1).min(last);
        let alpha = (position - index as f64).clamp(0.0, 1.0);
        let (a, b) = (&self.samples[index], &self.samples[next]);
        let mix = |x: f64, y: f64| x + (y - x) * alpha;
        PlaybackSample {
            pose: RenderPose::lerp(&a.pose, &b.pose, alpha),
            speed: mix(a.speed, b.speed),
            gear: if alpha < 0.5 { a.gear } else { b.gear },
            engine_rpm: mix(a.engine_rpm, b.engine_rpm),
            steering_wheel_angle: mix(a.steering_wheel_angle, b.steering_wheel_angle),
            steer: mix(a.steer, b.steer),
            throttle: mix(a.throttle, b.throttle),
            brake: mix(a.brake, b.brake),
            run_time_s: if a.run_time_s < 0.0 || b.run_time_s < 0.0 {
                a.run_time_s
            } else {
                mix(a.run_time_s, b.run_time_s)
            },
        }
    }

    /// Which cones are down at `time_s`.
    pub fn cones_at(&self, time_s: f64) -> &[bool] {
        let index = (time_s / self.sample_dt()).max(0.0) as usize;
        let at = self.cones.partition_point(|(i, _)| *i <= index);
        self.cones
            .get(at.saturating_sub(1))
            .map_or(&[], |(_, c)| c.as_slice())
    }
}
