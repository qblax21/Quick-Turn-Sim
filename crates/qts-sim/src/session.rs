// SPDX-License-Identifier: Apache-2.0
//! A session: one car on one track, stepped deterministically.

use crate::data::{DataError, DataRoot};
use crate::track::{segments_cross, GateKind, Track, CONE_RADIUS_M};
use qts_core::constants::{build_id, DEFAULT_TICK_RATE_HZ};
use qts_core::input::{buttons, InputFrame, GEAR_NO_REQUEST};
use qts_core::provenance::Provenance;
use qts_core::replay::ReplayHeader;
use qts_core::snapshot::{fnv1a64, Snapshot, SnapshotError, StateReader, StateWriter};
use qts_core::step::Simulation;
use qts_tire::{SurfaceParams, TireParams};
use qts_vehicle::{Assists, Car, CarParams, DriverControls, Environment};

/// Settings chosen when a session is created.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionConfig {
    /// Physics rate, ticks per second.
    pub tick_rate_hz: u32,
    /// Driver assists.
    pub assists: Assists,
    /// Starting tire temperature, °C. `None` starts the tires at the
    /// track's road temperature.
    pub tire_start_temp_c: Option<f64>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            tick_rate_hz: DEFAULT_TICK_RATE_HZ,
            assists: Assists {
                auto_clutch: true,
                auto_shift: false,
            },
            tire_start_temp_c: None,
        }
    }
}

/// Everything a session is built from, already loaded and validated.
#[derive(Clone, Debug)]
pub struct SessionAssets {
    /// Car parameters.
    pub car: CarParams,
    /// Front tire parameters.
    pub tire_front: TireParams,
    /// Rear tire parameters.
    pub tire_rear: TireParams,
    /// The track.
    pub track: Track,
    /// Hash over every data file that was loaded.
    pub content_hash: u64,
    /// Provenance of every physical parameter that was loaded.
    pub provenance: Vec<Provenance>,
}

impl SessionAssets {
    /// Loads a car, its tires, all surfaces and a track from `root`.
    pub fn load(root: &DataRoot, car_id: &str, track_id: &str) -> Result<Self, DataError> {
        let car = root.load_car(car_id)?;
        let tire_front = root.load_tire(&car.value.tire_front)?;
        let tire_rear = root.load_tire(&car.value.tire_rear)?;
        let surfaces = root.load_all_surfaces()?;
        let (track_def, track_hash) = root.load_track(track_id)?;

        let mut hashes = vec![
            car.content_hash,
            tire_front.content_hash,
            tire_rear.content_hash,
        ];
        let mut provenance = Vec::new();
        let prefix = |list: &[Provenance], label: &str, out: &mut Vec<Provenance>| {
            for p in list {
                out.push(Provenance {
                    path: format!("{label}:{}", p.path),
                    source: p.source.clone(),
                    confidence: p.confidence,
                });
            }
        };
        prefix(&car.provenance, &format!("car/{car_id}"), &mut provenance);
        prefix(
            &tire_front.provenance,
            &format!("tire/{}", tire_front.value.id),
            &mut provenance,
        );
        if tire_rear.value.id != tire_front.value.id {
            prefix(
                &tire_rear.provenance,
                &format!("tire/{}", tire_rear.value.id),
                &mut provenance,
            );
        }
        let mut surface_params: Vec<SurfaceParams> = Vec::new();
        for s in surfaces {
            hashes.push(s.content_hash);
            prefix(
                &s.provenance,
                &format!("surface/{}", s.value.id),
                &mut provenance,
            );
            surface_params.push(s.value);
        }
        hashes.push(track_hash);
        let track =
            Track::new(track_def, surface_params).map_err(|message| DataError::Invalid {
                path: root.path().join("tracks").join(format!("{track_id}.toml")),
                message,
            })?;
        let mut bytes = Vec::with_capacity(hashes.len() * 8);
        for h in hashes {
            bytes.extend_from_slice(&h.to_le_bytes());
        }
        Ok(Self {
            car: car.value,
            tire_front: tire_front.value,
            tire_rear: tire_rear.value,
            track,
            content_hash: fnv1a64(&bytes),
            provenance,
        })
    }
}

/// Lap and run timing, in ticks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timing {
    /// True between a start and the matching finish.
    pub running: bool,
    /// Tick at which the current run started.
    pub start_tick: u64,
    /// Duration of the last completed run, ticks. Zero if none.
    pub last_ticks: u64,
    /// Shortest completed run, ticks. Zero if none.
    pub best_ticks: u64,
    /// Completed runs.
    pub runs: u32,
    /// Cones knocked down in the current run.
    pub cones_hit: u32,
    /// Cones knocked down in the last completed run.
    pub last_cones_hit: u32,
    /// One entry per cone: true once knocked down.
    pub cone_down: Vec<bool>,
}

impl Timing {
    fn new(cones: usize) -> Self {
        Self {
            cone_down: vec![false; cones],
            ..Self::default()
        }
    }

    fn stand_cones_up(&mut self) {
        self.cone_down.iter_mut().for_each(|c| *c = false);
        self.cones_hit = 0;
    }
}

/// One car on one track.
#[derive(Debug)]
pub struct Session {
    config: SessionConfig,
    car: Car,
    track: Track,
    env: Environment,
    dt: f64,
    tick: u64,
    prev_buttons: u16,
    timing: Timing,
    content_hash: u64,
    half_length: f64,
    half_width: f64,
    box_center_x: f64,
}

impl Session {
    /// Builds a session with the car at the track's spawn point.
    pub fn new(assets: SessionAssets, config: SessionConfig) -> Result<Self, String> {
        if config.tick_rate_hz < 100 || config.tick_rate_hz > 10_000 {
            return Err(format!(
                "tick rate {} Hz is outside the supported range 100 to 10000",
                config.tick_rate_hz
            ));
        }
        let def = assets.track.def();
        let env = Environment {
            gravity: qts_core::constants::GRAVITY_M_S2,
            air_temp_c: def.air_temp_c,
            road_temp_c: def.road_temp_c,
        };
        let tire_temp = config.tire_start_temp_c.unwrap_or(def.road_temp_c);
        let body = &assets.car.body;
        let half_length = 0.5 * body.length_m;
        let half_width = 0.5 * body.width_m;
        let box_center_x =
            0.5 * (body.cg_to_front_axle_m - (body.wheelbase_m - body.cg_to_front_axle_m));
        let mut car = Car::new(
            assets.car,
            assets.tire_front,
            assets.tire_rear,
            env.gravity,
            tire_temp,
        )?;
        let (x, y, heading) = assets.track.spawn();
        car.place(x, y, heading, 0.0);
        let timing = Timing::new(assets.track.cones().len());
        Ok(Self {
            config,
            car,
            track: assets.track,
            env,
            dt: 1.0 / f64::from(config.tick_rate_hz),
            tick: 0,
            prev_buttons: 0,
            timing,
            content_hash: assets.content_hash,
            half_length,
            half_width,
            box_center_x,
        })
    }

    /// Loads the named car and track from `root` and builds a session.
    pub fn from_data(
        root: &DataRoot,
        car_id: &str,
        track_id: &str,
        config: SessionConfig,
    ) -> Result<Self, String> {
        let assets = SessionAssets::load(root, car_id, track_id).map_err(|e| e.to_string())?;
        Self::new(assets, config)
    }

    /// The car.
    pub fn car(&self) -> &Car {
        &self.car
    }

    /// Mutable access to the car, for scripted manoeuvres that set up an
    /// initial condition. Not for use while a replay is being recorded.
    pub fn car_mut(&mut self) -> &mut Car {
        &mut self.car
    }

    /// The track.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// Timing state.
    pub fn timing(&self) -> &Timing {
        &self.timing
    }

    /// Session settings.
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Air and road temperatures and gravity.
    pub fn environment(&self) -> &Environment {
        &self.env
    }

    /// Hash of the data files this session was built from.
    pub fn content_hash(&self) -> u64 {
        self.content_hash
    }

    /// Seconds elapsed in the current timed run, or zero if none is active.
    pub fn run_time(&self) -> f64 {
        if self.timing.running {
            (self.tick - self.timing.start_tick) as f64 * self.dt
        } else {
            0.0
        }
    }

    /// Converts a tick count to seconds at this session's rate.
    pub fn ticks_to_seconds(&self, ticks: u64) -> f64 {
        ticks as f64 * self.dt
    }

    /// A replay header describing this session.
    pub fn replay_header(&self, hash_interval: u32) -> ReplayHeader {
        ReplayHeader {
            tick_rate_hz: self.config.tick_rate_hz,
            car_id: self.car.params().id.clone(),
            track_id: self.track.def().id.clone(),
            content_hash: self.content_hash,
            build_id: build_id(),
            hash_interval,
            start_tick: self.tick,
        }
    }

    /// Puts the car back at the spawn point and clears the current run.
    fn reset_to_spawn(&mut self) {
        let (x, y, heading) = self.track.spawn();
        self.car.place(x, y, heading, 0.0);
        self.timing.running = false;
        self.timing.stand_cones_up();
    }

    fn update_timing(&mut self, before: [f64; 2], after: [f64; 2]) {
        for gate in self.track.gates() {
            if !segments_cross(before, after, gate.a, gate.b) {
                continue;
            }
            let finishing = matches!(gate.kind, GateKind::Finish | GateKind::StartFinish);
            let starting = matches!(gate.kind, GateKind::Start | GateKind::StartFinish);
            if finishing && self.timing.running {
                let ticks = self.tick - self.timing.start_tick;
                self.timing.last_ticks = ticks;
                self.timing.last_cones_hit = self.timing.cones_hit;
                if self.timing.best_ticks == 0 || ticks < self.timing.best_ticks {
                    self.timing.best_ticks = ticks;
                }
                self.timing.runs += 1;
                self.timing.running = false;
            }
            if starting {
                self.timing.running = true;
                self.timing.start_tick = self.tick;
                self.timing.stand_cones_up();
            }
        }
    }

    fn update_cones(&mut self) {
        if self.timing.cone_down.is_empty() {
            return;
        }
        let s = &self.car.state;
        let yaw = s.rot.yaw();
        let (sin, cos) = (qts_core::math::fm::sin(yaw), qts_core::math::fm::cos(yaw));
        // Centre of the body outline in the world.
        let cx = s.pos.x + cos * self.box_center_x;
        let cy = s.pos.y + sin * self.box_center_x;
        let reach = self.half_length + self.half_width + CONE_RADIUS_M;
        for (i, cone) in self.track.cones().iter().enumerate() {
            if self.timing.cone_down[i] {
                continue;
            }
            let (dx, dy) = (cone[0] - cx, cone[1] - cy);
            if dx.abs() > reach || dy.abs() > reach {
                continue;
            }
            let along = dx * cos + dy * sin;
            let across = -dx * sin + dy * cos;
            if along.abs() <= self.half_length + CONE_RADIUS_M
                && across.abs() <= self.half_width + CONE_RADIUS_M
            {
                self.timing.cone_down[i] = true;
                self.timing.cones_hit += 1;
            }
        }
    }

    /// True when every state value is finite.
    pub fn is_finite(&self) -> bool {
        self.car.is_finite()
    }
}

impl Simulation for Session {
    fn step(&mut self, input: &InputFrame) {
        let rising = input.buttons & !self.prev_buttons;
        self.prev_buttons = input.buttons;
        if rising & buttons::RESET != 0 {
            self.reset_to_spawn();
        }
        let controls = DriverControls {
            steer: input.steer_f64(),
            throttle: input.throttle_f64(),
            brake: input.brake_f64(),
            clutch: input.clutch_f64(),
            handbrake: input.handbrake_f64(),
            shift_up: rising & buttons::SHIFT_UP != 0,
            shift_down: rising & buttons::SHIFT_DOWN != 0,
            gear_request: if input.gear_request == GEAR_NO_REQUEST {
                None
            } else {
                Some(input.gear_request)
            },
        };
        let before = [self.car.state.pos.x, self.car.state.pos.y];
        self.car.step(
            &controls,
            &self.config.assists,
            &self.track,
            &self.env,
            self.dt,
        );
        self.tick += 1;
        let after = [self.car.state.pos.x, self.car.state.pos.y];
        self.update_timing(before, after);
        self.update_cones();
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn tick_rate_hz(&self) -> u32 {
        self.config.tick_rate_hz
    }
}

impl Snapshot for Session {
    fn save(&self, w: &mut StateWriter) {
        w.write_u64(self.tick);
        w.write_u16(self.prev_buttons);
        w.write_bool(self.config.assists.auto_clutch);
        w.write_bool(self.config.assists.auto_shift);
        let t = &self.timing;
        w.write_bool(t.running);
        w.write_u64(t.start_tick);
        w.write_u64(t.last_ticks);
        w.write_u64(t.best_ticks);
        w.write_u32(t.runs);
        w.write_u32(t.cones_hit);
        w.write_u32(t.last_cones_hit);
        w.write_u32(t.cone_down.len() as u32);
        for &down in &t.cone_down {
            w.write_bool(down);
        }
        self.car.save(w);
    }

    fn load(&mut self, r: &mut StateReader<'_>) -> Result<(), SnapshotError> {
        self.tick = r.read_u64()?;
        self.prev_buttons = r.read_u16()?;
        self.config.assists.auto_clutch = r.read_bool()?;
        self.config.assists.auto_shift = r.read_bool()?;
        let t = &mut self.timing;
        t.running = r.read_bool()?;
        t.start_tick = r.read_u64()?;
        t.last_ticks = r.read_u64()?;
        t.best_ticks = r.read_u64()?;
        t.runs = r.read_u32()?;
        t.cones_hit = r.read_u32()?;
        t.last_cones_hit = r.read_u32()?;
        let cones = r.read_u32()? as usize;
        if cones != t.cone_down.len() {
            return Err(SnapshotError::Invalid(
                "cone count does not match the track",
            ));
        }
        for down in &mut t.cone_down {
            *down = r.read_bool()?;
        }
        self.car.load(r)
    }
}
