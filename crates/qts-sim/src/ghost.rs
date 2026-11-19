// SPDX-License-Identifier: Apache-2.0
//! Ghost of the best lap: the car's pose recorded at a fixed interval
//! through the fastest timed run, played back beside the car on later runs.
//!
//! A ghost is a recording of poses, not a re-simulation. It reads the
//! session and never feeds anything back into it, so it has no effect on
//! determinism, and a saved ghost stays valid when the physics or the data
//! change (which a replay of inputs would not).

use crate::host::{RenderPose, WheelPose};
use crate::session::{Session, MAX_PLAUSIBLE_SPEED};
use qts_core::snapshot::{SnapshotError, StateReader, StateWriter};
use qts_core::step::Simulation;

/// Ticks between recorded poses.
pub const GHOST_SAMPLE_TICKS: u32 = 20;
/// Poses the delta search may advance through in one tick.
const DELTA_SEARCH_POSES: usize = 50;
/// Farther than this from the best lap's line, no delta is given, m.
const DELTA_MAX_GAP_M: f64 = 40.0;
/// Marks a ghost file.
const MAGIC: &[u8; 4] = b"GLAP";
/// Version of the ghost file layout.
const FORMAT_VERSION: u32 = 2;

/// One recorded lap.
#[derive(Clone, Debug, PartialEq)]
pub struct GhostLap {
    /// Car the lap was driven in.
    pub car_id: String,
    /// Track the lap was driven on.
    pub track_id: String,
    /// Physics rate of the session that recorded it, ticks per second.
    pub tick_rate_hz: u32,
    /// Duration of the lap, ticks.
    pub lap_ticks: u64,
    /// Ticks between poses.
    pub sample_ticks: u32,
    /// Poses from the start of the lap onward.
    pub poses: Vec<RenderPose>,
    /// Setup the lap was driven with, as canonical text (empty for the car
    /// as delivered). Files from before setups existed read as empty.
    pub setup: String,
}

impl GhostLap {
    /// Lap time, s.
    pub fn seconds(&self) -> f64 {
        self.lap_ticks as f64 / f64::from(self.tick_rate_hz.max(1))
    }

    /// Pose `run_ticks` ticks into the lap (fractions allowed), or `None`
    /// once the lap is over.
    pub fn pose_at(&self, run_ticks: f64) -> Option<RenderPose> {
        if self.poses.is_empty() || run_ticks < 0.0 || run_ticks > self.lap_ticks as f64 {
            return None;
        }
        let position = run_ticks / f64::from(self.sample_ticks.max(1));
        let index = position.floor() as usize;
        let last = self.poses.len() - 1;
        if index >= last {
            return Some(self.poses[last]);
        }
        Some(RenderPose::lerp(
            &self.poses[index],
            &self.poses[index + 1],
            position - index as f64,
        ))
    }

    /// True if this lap can be shown in `session`.
    pub fn fits(&self, session: &Session) -> bool {
        self.car_id == session.car().params().id
            && self.track_id == session.track().def().id
            && self.tick_rate_hz == session.tick_rate_hz()
    }

    /// Whether the lap is a real lap of `session`'s track: the poses cover
    /// the lap's duration, pass every checkpoint forward and in order, cover
    /// most of the course, and the time is plausible. Ghost files written
    /// before laps were checked can fail this; they are ignored.
    pub fn is_valid_lap(&self, session: &Session) -> bool {
        let track = session.track();
        let samples = self.lap_ticks / u64::from(self.sample_ticks.max(1)) + 1;
        let seconds = self.seconds();
        let points: Vec<[f64; 2]> = self.poses.iter().map(|p| [p.pos[0], p.pos[1]]).collect();
        self.lap_ticks > 0
            && (self.poses.len() as u64).abs_diff(samples) <= 2
            && seconds * MAX_PLAUSIBLE_SPEED >= track.timed_length_m()
            && track.run_follows_course(&points)
    }

    /// Serialises the lap. Positions keep full precision; orientation and
    /// wheel values are stored as 32-bit floats.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = StateWriter::recording();
        w.write_raw(MAGIC);
        w.write_u32(FORMAT_VERSION);
        w.write_str(&self.car_id);
        w.write_str(&self.track_id);
        w.write_u32(self.tick_rate_hz);
        w.write_u64(self.lap_ticks);
        w.write_u32(self.sample_ticks);
        w.write_u32(self.poses.len() as u32);
        let small = |w: &mut StateWriter, v: f64| w.write_u32((v as f32).to_bits());
        for pose in &self.poses {
            w.write_f64s(&pose.pos);
            for r in pose.rot {
                small(&mut w, r);
            }
            for wheel in &pose.wheels {
                for c in wheel.center_body {
                    small(&mut w, c);
                }
                small(&mut w, wheel.steer_angle);
                small(&mut w, wheel.spin_angle);
            }
        }
        w.write_str(&self.setup);
        w.into_bytes()
    }

    /// Reads a lap written by [`GhostLap::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let mut r = StateReader::new(bytes);
        let bad = SnapshotError::Invalid;
        if r.read_raw(4)? != MAGIC {
            return Err(bad("not a ghost lap file"));
        }
        let version = r.read_u32()?;
        if !(1..=FORMAT_VERSION).contains(&version) {
            return Err(bad("unsupported ghost lap version"));
        }
        let car_id = r.read_string()?;
        let track_id = r.read_string()?;
        let tick_rate_hz = r.read_u32()?;
        let lap_ticks = r.read_u64()?;
        let sample_ticks = r.read_u32()?;
        let count = r.read_u32()? as usize;
        if tick_rate_hz == 0 || sample_ticks == 0 || count > r.remaining() {
            return Err(bad("implausible ghost lap header"));
        }
        let small = |r: &mut StateReader| -> Result<f64, SnapshotError> {
            Ok(f64::from(f32::from_bits(r.read_u32()?)))
        };
        let mut poses = Vec::with_capacity(count);
        for i in 0..count {
            let mut pose = RenderPose {
                tick: i as u64 * u64::from(sample_ticks),
                ..RenderPose::default()
            };
            r.read_f64s(&mut pose.pos)?;
            for q in &mut pose.rot {
                *q = small(&mut r)?;
            }
            for wheel in &mut pose.wheels {
                let mut out = WheelPose::default();
                for c in &mut out.center_body {
                    *c = small(&mut r)?;
                }
                out.steer_angle = small(&mut r)?;
                out.spin_angle = small(&mut r)?;
                *wheel = out;
            }
            if !pose
                .pos
                .iter()
                .chain(pose.rot.iter())
                .all(|v| v.is_finite())
            {
                return Err(bad("non-finite ghost pose"));
            }
            poses.push(pose);
        }
        let setup = if version >= 2 {
            r.read_string()?
        } else {
            String::new()
        };
        r.finish()?;
        Ok(Self {
            car_id,
            track_id,
            tick_rate_hz,
            lap_ticks,
            sample_ticks,
            poses,
            setup,
        })
    }
}

/// Watches a session tick by tick, records each timed run and keeps the
/// fastest as the ghost.
#[derive(Clone, Debug, Default)]
pub struct GhostRecorder {
    best: Option<GhostLap>,
    current: Vec<RenderPose>,
    start_tick: u64,
    runs_seen: u32,
    /// Pose of the best lap nearest to where the car is now. It only moves
    /// forward through a run.
    cursor: usize,
    /// Time gained (negative) or lost against the best lap at the car's
    /// present position, s.
    delta_s: Option<f64>,
}

impl GhostRecorder {
    /// Starts with an earlier best lap, if there is one.
    pub fn new(best: Option<GhostLap>) -> Self {
        Self {
            best,
            ..Self::default()
        }
    }

    /// The lap being shown, if any.
    pub fn best(&self) -> Option<&GhostLap> {
        self.best.as_ref()
    }

    /// Call once after every tick with the pose of that tick. Returns true
    /// when the run that just finished became the new best lap.
    pub fn observe(&mut self, session: &Session, pose: &RenderPose) -> bool {
        let timing = session.timing();
        let mut improved = false;
        if timing.runs != self.runs_seen {
            // A run finished on this tick; `current` holds it.
            if timing.runs > self.runs_seen
                && !self.current.is_empty()
                && self
                    .best
                    .as_ref()
                    .is_none_or(|b| timing.last_ticks < b.lap_ticks)
            {
                // The finishing pose closes the lap.
                self.current.push(*pose);
                let lap = GhostLap {
                    car_id: session.car().params().id.clone(),
                    track_id: session.track().def().id.clone(),
                    tick_rate_hz: session.tick_rate_hz(),
                    lap_ticks: timing.last_ticks,
                    sample_ticks: GHOST_SAMPLE_TICKS,
                    poses: std::mem::take(&mut self.current),
                    setup: session.setup().to_text(),
                };
                // The session only counts runs that took the course; this
                // second check guards the file against anything it missed.
                if lap.is_valid_lap(session) {
                    self.best = Some(lap);
                    improved = true;
                    self.cursor = 0;
                }
            }
            self.runs_seen = timing.runs;
            self.current.clear();
        }
        if !timing.running {
            self.current.clear();
            self.delta_s = None;
            return improved;
        }
        if timing.start_tick != self.start_tick {
            self.start_tick = timing.start_tick;
            self.current.clear();
            self.cursor = 0;
        }
        let into_run = session.tick().saturating_sub(timing.start_tick);
        if into_run % u64::from(GHOST_SAMPLE_TICKS) == 0 {
            self.current.push(*pose);
        }
        self.delta_s = self.delta(session, pose, into_run);
        improved
    }

    /// Time against the best lap at the car's present position: how long
    /// this run has taken to get here, less how long the best lap took to
    /// reach the nearest point on its own line. Negative is ahead. `None`
    /// without a best lap, outside a run, or when the car is far from the
    /// line the best lap took.
    pub fn delta_s(&self) -> Option<f64> {
        self.delta_s
    }

    fn delta(&mut self, session: &Session, pose: &RenderPose, into_run: u64) -> Option<f64> {
        let best = self.best.as_ref()?;
        let poses = &best.poses;
        if poses.len() < 2 {
            return None;
        }
        let gap2 = |p: &RenderPose| {
            let (dx, dy) = (p.pos[0] - pose.pos[0], p.pos[1] - pose.pos[1]);
            dx * dx + dy * dy
        };
        // Follow the best lap forward while its next pose is nearer.
        let last = poses.len() - 1;
        self.cursor = self.cursor.min(last);
        let mut steps = 0;
        while self.cursor < last
            && steps < DELTA_SEARCH_POSES
            && gap2(&poses[self.cursor + 1]) <= gap2(&poses[self.cursor])
        {
            self.cursor += 1;
            steps += 1;
        }
        if gap2(&poses[self.cursor]) > DELTA_MAX_GAP_M * DELTA_MAX_GAP_M {
            return None;
        }
        // Place the car between that pose and its nearer neighbour.
        let i = self.cursor;
        let j = if i == last || (i > 0 && gap2(&poses[i - 1]) < gap2(&poses[i + 1])) {
            i - 1
        } else {
            i + 1
        };
        let (a, b) = (i.min(j), i.max(j));
        let (sx, sy) = (
            poses[b].pos[0] - poses[a].pos[0],
            poses[b].pos[1] - poses[a].pos[1],
        );
        let len2 = sx * sx + sy * sy;
        let along = if len2 > 0.0 {
            (((pose.pos[0] - poses[a].pos[0]) * sx + (pose.pos[1] - poses[a].pos[1]) * sy) / len2)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        let best_ticks = (a as f64 + along) * f64::from(best.sample_ticks);
        Some((into_run as f64 - best_ticks) / f64::from(session.tick_rate_hz().max(1)))
    }

    /// Where the ghost is right now, if a run is in progress and a best lap
    /// exists. `fraction` is how far the display is between this tick and
    /// the next.
    pub fn pose_now(&self, session: &Session, fraction: f64) -> Option<RenderPose> {
        let timing = session.timing();
        if !timing.running {
            return None;
        }
        let into_run = session.tick().saturating_sub(timing.start_tick) as f64 + fraction;
        self.best.as_ref()?.pose_at(into_run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lap(poses: usize) -> GhostLap {
        GhostLap {
            car_id: "car".into(),
            track_id: "track".into(),
            tick_rate_hz: 1000,
            lap_ticks: (poses as u64 - 1) * 20 + 7,
            sample_ticks: 20,
            setup: "name = test\nfront.camber_deg = -2\n".into(),
            poses: (0..poses)
                .map(|i| RenderPose {
                    tick: i as u64 * 20,
                    pos: [i as f64 * 2.0, 0.5, 0.25],
                    ..RenderPose::default()
                })
                .collect(),
        }
    }

    #[test]
    fn pose_is_interpolated_and_ends_with_the_lap() {
        let g = lap(11);
        assert_eq!(g.pose_at(0.0).unwrap().pos[0], 0.0);
        assert!((g.pose_at(30.0).unwrap().pos[0] - 3.0).abs() < 1e-12);
        assert_eq!(g.pose_at(205.0).unwrap().pos[0], 20.0);
        assert!(g.pose_at(208.0).is_none());
        assert!(g.pose_at(-1.0).is_none());
        assert!((g.seconds() - 0.207).abs() < 1e-12);
    }

    #[test]
    fn a_lap_survives_a_round_trip_and_bad_files_are_refused() {
        let mut g = lap(5);
        g.poses[2].wheels[1].spin_angle = 1.25;
        g.poses[3].rot = [0.5, 0.5, 0.5, 0.5];
        let bytes = g.to_bytes();
        assert_eq!(GhostLap::from_bytes(&bytes).unwrap(), g);
        assert!(GhostLap::from_bytes(&bytes[..bytes.len() - 3]).is_err());
        assert!(GhostLap::from_bytes(b"nope").is_err());
        let mut wrong = bytes.clone();
        wrong[4] = 9;
        assert!(GhostLap::from_bytes(&wrong).is_err());
    }
}
