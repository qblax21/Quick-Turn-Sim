// SPDX-License-Identifier: Apache-2.0
//! The real-time host.
//!
//! A [`RealtimeHost`] owns a thread that steps a [`Session`] against the
//! wall clock at the physics rate, runs the force-feedback pipeline every
//! tick, talks to the steering device, and publishes poses for the renderer
//! to interpolate. Physics and force feedback therefore run at the physics
//! rate whatever the frame rate is.
//!
//! This is host code: it is the only place in the workspace that reads a
//! clock. The clock decides how many ticks to run and nothing else; no time
//! value ever reaches [`Simulation::step`].

use crate::ffb_log::{FfbLogStatus, FfbLogger, CLIP_THRESHOLD};
use crate::ghost::{GhostLap, GhostRecorder};
use crate::session::Session;
use crate::telemetry;
use qts_core::input::{InputFrame, AXIS_MAX};
use qts_core::replay::ReplayRecorder;
use qts_core::step::{FixedStepDriver, Simulation};
use qts_ffb::{DeviceInfo, FfbConfig, FfbDevice, FfbInput, FfbPipeline, FfbSample};
use qts_telemetry::TelemetryWriter;
use qts_vehicle::drivetrain::DriveTelemetry;
use qts_vehicle::WheelOutput;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long the sim thread sleeps between checks of the clock.
const IDLE_SLEEP: Duration = Duration::from_micros(250);
/// Longest stretch of ticks an input change is spread over, as a fraction
/// of a second.
const MAX_INPUT_RAMP_S: f64 = 0.05;

/// Settings for a host.
#[derive(Clone, Debug)]
pub struct HostOptions {
    /// Force-feedback pipeline configuration.
    pub ffb: FfbConfig,
    /// Most ticks run in one wake-up. Bounds catch-up after a stall.
    pub max_catch_up_ticks: u32,
    /// Spread each change of the analog inputs over the interval between
    /// input updates, so frame-rate input does not reach the physics as a
    /// staircase.
    pub interpolate_inputs: bool,
    /// File holding the best-lap ghost for this car and track. It is read
    /// at start if it exists and fits the session, and rewritten whenever
    /// a faster lap is driven. `None` keeps the ghost in memory only.
    pub ghost_path: Option<PathBuf>,
}

/// Pose of one wheel for drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelPose {
    /// Wheel centre in the car's body frame, m.
    pub center_body: [f64; 3],
    /// Steer angle relative to the body, rad.
    pub steer_angle: f64,
    /// Rotation about the spin axis, rad.
    pub spin_angle: f64,
}

/// Pose of the car for drawing, in the simulation's world frame (x forward,
/// y left, z up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderPose {
    /// Tick this pose belongs to.
    pub tick: u64,
    /// Position of the sprung centre of gravity, m.
    pub pos: [f64; 3],
    /// Orientation as a quaternion `[w, x, y, z]`.
    pub rot: [f64; 4],
    /// Wheel poses.
    pub wheels: [WheelPose; 4],
}

impl Default for RenderPose {
    fn default() -> Self {
        Self {
            tick: 0,
            pos: [0.0; 3],
            rot: [1.0, 0.0, 0.0, 0.0],
            wheels: [WheelPose::default(); 4],
        }
    }
}

impl RenderPose {
    pub(crate) fn of(session: &Session) -> Self {
        let car = session.car();
        let s = &car.state;
        let mut wheels = [WheelPose::default(); 4];
        for (i, w) in wheels.iter_mut().enumerate() {
            let out = &car.output().wheels[i];
            *w = WheelPose {
                center_body: out.center_body,
                steer_angle: out.steer_angle,
                spin_angle: s.wheel_angle[i],
            };
        }
        Self {
            tick: session.tick(),
            pos: s.pos.to_array(),
            rot: [s.rot.w, s.rot.x, s.rot.y, s.rot.z],
            wheels,
        }
    }

    /// Blends two poses; `alpha` 0 gives `a`, 1 gives `b`.
    pub fn lerp(a: &RenderPose, b: &RenderPose, alpha: f64) -> RenderPose {
        let t = alpha.clamp(0.0, 1.0);
        let mix = |x: f64, y: f64| x + (y - x) * t;
        let mut out = *b;
        for i in 0..3 {
            out.pos[i] = mix(a.pos[i], b.pos[i]);
        }
        // Normalised linear blend along the shorter arc; poses one tick
        // apart are close enough that this is indistinguishable from slerp.
        let dot: f64 = (0..4).map(|i| a.rot[i] * b.rot[i]).sum();
        let sign = if dot < 0.0 { -1.0 } else { 1.0 };
        let mut norm = 0.0;
        for i in 0..4 {
            out.rot[i] = mix(a.rot[i], sign * b.rot[i]);
            norm += out.rot[i] * out.rot[i];
        }
        let norm = norm.sqrt();
        if norm > 1e-12 {
            for r in &mut out.rot {
                *r /= norm;
            }
        } else {
            out.rot = b.rot;
        }
        for i in 0..4 {
            let (wa, wb) = (&a.wheels[i], &b.wheels[i]);
            for k in 0..3 {
                out.wheels[i].center_body[k] = mix(wa.center_body[k], wb.center_body[k]);
            }
            out.wheels[i].steer_angle = mix(wa.steer_angle, wb.steer_angle);
            // Spin angles wrap at ±π; blend across the wrap.
            let mut delta = wb.spin_angle - wa.spin_angle;
            if delta > std::f64::consts::PI {
                delta -= 2.0 * std::f64::consts::PI;
            } else if delta < -std::f64::consts::PI {
                delta += 2.0 * std::f64::consts::PI;
            }
            out.wheels[i].spin_angle = wa.spin_angle + delta * t;
        }
        out
    }
}

/// Values for the HUD and debug overlay, from the latest tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HudState {
    /// Tick these values belong to.
    pub tick: u64,
    /// Speed over the ground, m/s.
    pub speed: f64,
    /// Drivetrain values (gear, engine speed, battery).
    pub drive: DriveTelemetry,
    /// Per-wheel values (slip, load, temperatures).
    pub wheels: [WheelOutput; 4],
    /// Raw steering torque, N·m.
    pub steering_torque_nm: f64,
    /// Hits on trees and barriers so far.
    pub impact_count: u32,
    /// Impulse of the last hit, N·s.
    pub impact_impulse_ns: f64,
    /// Steering-wheel angle the sim is using, rad.
    pub steering_wheel_angle: f64,
    /// Acceleration in body axes, m/s².
    pub accel_body: [f64; 3],
    /// True while a timed run is in progress.
    pub run_active: bool,
    /// Time in the current run, s.
    pub run_time_s: f64,
    /// Duration of the last completed run, s. Zero if none.
    pub last_time_s: f64,
    /// Best completed run, s. Zero if none.
    pub best_time_s: f64,
    /// Completed runs.
    pub runs: u32,
    /// Cones down in the current run.
    pub cones_hit: u32,
    /// Cones down in the last completed run.
    pub last_cones_hit: u32,
    /// Runs that reached the finish without taking the course.
    pub invalid_runs: u32,
}

impl HudState {
    pub(crate) fn of(session: &Session) -> Self {
        let out = session.car().output();
        let t = session.timing();
        Self {
            tick: session.tick(),
            speed: out.speed,
            drive: out.drive,
            wheels: out.wheels,
            steering_torque_nm: out.steering_torque_nm,
            impact_count: session.impact().0,
            impact_impulse_ns: session.impact().1,
            steering_wheel_angle: out.steering_wheel_angle,
            accel_body: out.accel_body,
            run_active: t.running,
            run_time_s: session.run_time(),
            last_time_s: session.ticks_to_seconds(t.last_ticks),
            best_time_s: session.ticks_to_seconds(t.best_ticks),
            runs: t.runs,
            cones_hit: t.cones_hit,
            last_cones_hit: t.last_cones_hit,
            invalid_runs: t.invalid_runs,
        }
    }
}

/// Health and diagnostics of a running host.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HostStatus {
    /// The open steering device, if any.
    pub device: Option<DeviceInfo>,
    /// Last device or file error, cleared when the next related command
    /// succeeds.
    pub last_error: Option<String>,
    /// True while force output is enabled.
    pub ffb_enabled: bool,
    /// Latest force-feedback pipeline sample.
    pub ffb: FfbSample,
    /// Share of recent ticks (about the last second) in which the soft clip
    /// was compressing the output.
    pub clip_fraction: f64,
    /// Largest `|requested|` force over the last two seconds or so, as a
    /// fraction of the device's peak. Above the soft-clip knee the wheel is
    /// being compressed; above one it is saturated.
    pub peak_requested: f64,
    /// The force-feedback log, if one was asked for.
    pub ffb_log: FfbLogStatus,
    /// Ticks simulated per second of real time, measured.
    pub measured_tick_rate_hz: f64,
    /// Most ticks run in a single wake-up during the last second. Above a
    /// handful means the thread is being scheduled late.
    pub worst_wake_ticks: u32,
    /// Real time discarded because the host fell too far behind, s.
    pub dropped_time_s: f64,
    /// Rows written to the telemetry file, if one is open.
    pub telemetry_rows: Option<u64>,
    /// Frames in the replay being recorded, if any.
    pub replay_frames: Option<usize>,
    /// True while the simulation is paused.
    pub paused: bool,
    /// Angle of the physical wheel, rad, positive left, if a device reports
    /// it.
    pub device_angle_rad: Option<f64>,
}

/// Instructions for the sim thread.
#[derive(Clone, Debug)]
pub enum HostCommand {
    /// Open a steering device by identifier. `window` is the native handle
    /// of the application window.
    OpenDevice {
        /// Identifier from `qts_ffb::platform::enumerate`.
        id: String,
        /// Native window handle, if the platform needs one.
        window: Option<isize>,
    },
    /// Release the steering device.
    CloseDevice,
    /// Replace the pipeline configuration.
    SetFfbConfig(Box<FfbConfig>),
    /// Enable or disable force output without closing the device.
    SetFfbEnabled(bool),
    /// Take steering directly from the open device at the physics rate,
    /// mapping its angle one-to-one onto the car's steering wheel.
    SetDirectSteering(bool),
    /// Start writing telemetry to a file at about the given sample rate.
    StartTelemetry {
        /// Output path.
        path: PathBuf,
        /// Rows per second.
        sample_rate_hz: f64,
    },
    /// Close the telemetry file.
    StopTelemetry,
    /// Start recording a replay from the current state.
    StartReplay {
        /// Ticks between stored state hashes.
        hash_interval: u32,
    },
    /// Stop recording and write the replay to a file.
    StopReplay {
        /// Output path.
        path: PathBuf,
    },
    /// Write the replay recorded so far to a file and keep recording.
    SaveReplay {
        /// Output path.
        path: PathBuf,
    },
    /// Pause or resume stepping.
    SetPaused(bool),
    /// Log the force-feedback signal for the next full lap into a CSV file.
    StartFfbLog {
        /// Output path.
        path: PathBuf,
    },
    /// Stop the force-feedback log, saving a lap in progress as far as it
    /// went.
    StopFfbLog,
}

#[derive(Default)]
struct InputSlot {
    frame: InputFrame,
    sequence: u64,
}

#[derive(Default)]
struct Published {
    previous: RenderPose,
    current: RenderPose,
    alpha: f64,
    hud: HudState,
    cones_down: Vec<bool>,
    ghost: Option<RenderPose>,
    ghost_time_s: f64,
    ghost_delta_s: Option<f64>,
}

struct Shared {
    stop: AtomicBool,
    input: Mutex<InputSlot>,
    commands: Mutex<Vec<HostCommand>>,
    published: Mutex<Published>,
    status: Mutex<HostStatus>,
}

/// Locks a mutex, recovering the data if another thread panicked while
/// holding it. The protected values are plain data that stay usable.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Builds the pipeline input for the session's last tick.
pub fn ffb_input(session: &Session, device_angle_rad: Option<f64>) -> FfbInput {
    let out = session.car().output();
    let surfaces = session.track().surfaces();
    let on_kerb = |i: usize| {
        surfaces
            .get(usize::from(out.wheels[i].surface_index))
            .is_some_and(|s| s.id == "kerb")
    };
    let kerb_contact = 0.5 * (f64::from(u8::from(on_kerb(0))) + f64::from(u8::from(on_kerb(1))));
    FfbInput {
        // A hit on a tree or barrier adds a short kick to the column torque.
        steering_torque_nm: out.steering_torque_nm + session.impact_kick_nm(),
        device_angle_rad,
        front_load_difference_n: out.wheels[0].load - out.wheels[1].load,
        kerb_contact,
    }
}

/// A session running in real time on its own thread.
pub struct RealtimeHost {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<Session>>,
}

impl RealtimeHost {
    /// Starts stepping `session` in real time.
    pub fn start(session: Session, options: HostOptions) -> Result<Self, String> {
        let pipeline = FfbPipeline::new(options.ffb.clone(), session.tick_rate_hz())?;
        let pose = RenderPose::of(&session);
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            input: Mutex::new(InputSlot::default()),
            commands: Mutex::new(Vec::new()),
            published: Mutex::new(Published {
                previous: pose,
                current: pose,
                alpha: 1.0,
                hud: HudState::of(&session),
                cones_down: session.timing().cone_down.clone(),
                ghost: None,
                ghost_time_s: 0.0,
                ghost_delta_s: None,
            }),
            status: Mutex::new(HostStatus {
                ffb_enabled: true,
                ..HostStatus::default()
            }),
        });
        let thread_shared = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("sim".into())
            .spawn(move || SimThread::new(session, pipeline, options, thread_shared).run())
            .map_err(|e| format!("could not start the simulation thread: {e}"))?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Supplies the latest driver input. Call as often as input is sampled.
    pub fn set_input(&self, frame: InputFrame) {
        let mut slot = lock(&self.shared.input);
        slot.frame = frame;
        slot.sequence += 1;
    }

    /// Queues a command for the sim thread.
    pub fn send(&self, command: HostCommand) {
        lock(&self.shared.commands).push(command);
    }

    /// The pose to draw now (interpolated between the last two ticks) and
    /// the latest HUD values.
    pub fn render(&self) -> (RenderPose, HudState) {
        let p = lock(&self.shared.published);
        (RenderPose::lerp(&p.previous, &p.current, p.alpha), p.hud)
    }

    /// Pose of the best-lap ghost at this moment of the current run, if a
    /// run is in progress and a best lap exists, and that lap's time in
    /// seconds (zero if there is none).
    pub fn ghost(&self) -> (Option<RenderPose>, f64) {
        let p = lock(&self.shared.published);
        (p.ghost, p.ghost_time_s)
    }

    /// Time against the best lap at the car's present position, s;
    /// negative is ahead. `None` when there is nothing to compare with.
    pub fn ghost_delta(&self) -> Option<f64> {
        lock(&self.shared.published).ghost_delta_s
    }

    /// Which cones are currently knocked down, in track cone order.
    pub fn cones_down(&self) -> Vec<bool> {
        lock(&self.shared.published).cones_down.clone()
    }

    /// Current diagnostics.
    pub fn status(&self) -> HostStatus {
        lock(&self.shared.status).clone()
    }

    /// Stops the thread and returns the session in its final state.
    pub fn stop(mut self) -> Option<Session> {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.thread.take().and_then(|t| t.join().ok())
    }
}

impl Drop for RealtimeHost {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct SimThread {
    session: Session,
    pipeline: FfbPipeline,
    options: HostOptions,
    shared: Arc<Shared>,
    device: Option<Box<dyn FfbDevice>>,
    ffb_enabled: bool,
    direct_steering: bool,
    paused: bool,
    telemetry: Option<TelemetryWriter<BufWriter<File>>>,
    recorder: Option<ReplayRecorder>,
    ghost: GhostRecorder,
    row: Vec<f64>,
    // Input interpolation.
    seen_sequence: u64,
    ticks_since_input: u32,
    ramp_from: InputFrame,
    ramp_to: InputFrame,
    ramp_len: u32,
    ramp_pos: u32,
    // Device angle, ramped between reads.
    device_divider: u32,
    device_phase: u32,
    angle_from: f64,
    angle_to: f64,
    angle_valid: bool,
    // Published poses.
    previous: RenderPose,
    current: RenderPose,
    last_sample: FfbSample,
    last_error: Option<String>,
    clip_fraction: f64,
    // Peak requested force: the current and previous one-second windows.
    peak_now: f64,
    peak_before: f64,
    peak_ticks: u32,
    ffb_log: Option<FfbLogger>,
    ffb_log_status: FfbLogStatus,
}

fn ramp_axis(from: i16, to: i16, pos: u32, len: u32) -> i16 {
    if pos >= len {
        return to;
    }
    let t = f64::from(pos) / f64::from(len);
    (f64::from(from) + (f64::from(to) - f64::from(from)) * t).round() as i16
}

impl SimThread {
    fn new(
        session: Session,
        pipeline: FfbPipeline,
        options: HostOptions,
        shared: Arc<Shared>,
    ) -> Self {
        let pose = RenderPose::of(&session);
        let rate = f64::from(session.tick_rate_hz());
        let device_divider = (rate / options.ffb.device_update_rate_hz).round().max(1.0) as u32;
        // A missing, unreadable or mismatched ghost file just means no ghost.
        let saved = options
            .ghost_path
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| GhostLap::from_bytes(&bytes).ok())
            .filter(|lap| lap.fits(&session) && lap.is_valid_lap(&session));
        Self {
            ghost: GhostRecorder::new(saved),
            session,
            pipeline,
            options,
            shared,
            device: None,
            ffb_enabled: true,
            direct_steering: false,
            paused: false,
            telemetry: None,
            recorder: None,
            row: Vec::new(),
            seen_sequence: 0,
            ticks_since_input: 0,
            ramp_from: InputFrame::default(),
            ramp_to: InputFrame::default(),
            ramp_len: 1,
            ramp_pos: 1,
            device_divider,
            device_phase: 0,
            angle_from: 0.0,
            angle_to: 0.0,
            angle_valid: false,
            previous: pose,
            current: pose,
            last_sample: FfbSample::default(),
            last_error: None,
            clip_fraction: 0.0,
            peak_now: 0.0,
            peak_before: 0.0,
            peak_ticks: 0,
            ffb_log: None,
            ffb_log_status: FfbLogStatus::Off,
        }
    }

    fn handle(&mut self, command: HostCommand) {
        match command {
            HostCommand::OpenDevice { id, window } => {
                self.close_device();
                match qts_ffb::platform::open(&id, window) {
                    Ok(device) => {
                        self.device = Some(device);
                        self.last_error = None;
                    }
                    Err(e) => self.last_error = Some(e.to_string()),
                }
            }
            HostCommand::CloseDevice => self.close_device(),
            HostCommand::SetFfbConfig(config) => {
                let rate = f64::from(self.session.tick_rate_hz());
                self.device_divider = (rate / config.device_update_rate_hz).round().max(1.0) as u32;
                match self.pipeline.set_config(*config) {
                    Ok(()) => self.last_error = None,
                    Err(e) => self.last_error = Some(format!("force-feedback configuration: {e}")),
                }
            }
            HostCommand::SetFfbEnabled(enabled) => {
                self.ffb_enabled = enabled;
                if !enabled {
                    self.silence_device();
                }
            }
            HostCommand::SetDirectSteering(enabled) => self.direct_steering = enabled,
            HostCommand::StartTelemetry {
                path,
                sample_rate_hz,
            } => {
                let rate = f64::from(self.session.tick_rate_hz());
                let decimation = (rate / sample_rate_hz.max(1.0)).round().max(1.0) as u32;
                let result = File::create(&path).and_then(|file| {
                    TelemetryWriter::new(
                        BufWriter::new(file),
                        &telemetry::generator(),
                        &telemetry::metadata(&self.session, rate / f64::from(decimation)),
                        &telemetry::channels(),
                        decimation,
                    )
                });
                match result {
                    Ok(writer) => {
                        self.telemetry = Some(writer);
                        self.last_error = None;
                    }
                    Err(e) => self.last_error = Some(format!("telemetry {}: {e}", path.display())),
                }
            }
            HostCommand::StopTelemetry => self.close_telemetry(),
            HostCommand::StartReplay { hash_interval } => {
                let header = self.session.replay_header(hash_interval);
                self.recorder = Some(ReplayRecorder::start(header, &self.session));
            }
            HostCommand::SaveReplay { path } => {
                if let Some(recorder) = &self.recorder {
                    let bytes = recorder.clone().finish().to_bytes();
                    match std::fs::write(&path, bytes) {
                        Ok(()) => self.last_error = None,
                        Err(e) => self.last_error = Some(format!("replay {}: {e}", path.display())),
                    }
                }
            }
            HostCommand::StopReplay { path } => {
                if let Some(recorder) = self.recorder.take() {
                    match std::fs::write(&path, recorder.finish().to_bytes()) {
                        Ok(()) => self.last_error = None,
                        Err(e) => self.last_error = Some(format!("replay {}: {e}", path.display())),
                    }
                }
            }
            HostCommand::StartFfbLog { path } => {
                self.ffb_log = Some(FfbLogger::arm(path, &self.session));
            }
            HostCommand::StopFfbLog => {
                if let Some(log) = self.ffb_log.as_mut() {
                    log.stop(&self.session, self.pipeline.config());
                }
            }
            HostCommand::SetPaused(paused) => {
                self.paused = paused;
                if paused {
                    self.silence_device();
                }
            }
        }
    }

    fn silence_device(&mut self) {
        if let Some(device) = self.device.as_mut() {
            let _ = device.stop();
        }
    }

    fn close_device(&mut self) {
        self.silence_device();
        self.device = None;
        self.angle_valid = false;
    }

    fn close_telemetry(&mut self) {
        if let Some(writer) = self.telemetry.take() {
            if let Err(e) = writer.finish() {
                self.last_error = Some(format!("telemetry: {e}"));
            }
        }
    }

    /// The input frame for the next tick.
    fn next_frame(&mut self) -> InputFrame {
        let (target, sequence) = {
            let slot = lock(&self.shared.input);
            (slot.frame, slot.sequence)
        };
        if sequence != self.seen_sequence {
            self.seen_sequence = sequence;
            let rate = f64::from(self.session.tick_rate_hz());
            let longest = (MAX_INPUT_RAMP_S * rate).max(1.0) as u32;
            self.ramp_from = self.interpolated();
            self.ramp_to = target;
            self.ramp_len = if self.options.interpolate_inputs {
                self.ticks_since_input.clamp(1, longest)
            } else {
                1
            };
            self.ramp_pos = 0;
            self.ticks_since_input = 0;
        }
        self.ticks_since_input = self.ticks_since_input.saturating_add(1);
        self.ramp_pos = self.ramp_pos.saturating_add(1);
        self.interpolated()
    }

    fn interpolated(&self) -> InputFrame {
        let (a, b, pos, len) = (&self.ramp_from, &self.ramp_to, self.ramp_pos, self.ramp_len);
        InputFrame {
            steer: ramp_axis(a.steer, b.steer, pos, len),
            throttle: ramp_axis(a.throttle, b.throttle, pos, len),
            brake: ramp_axis(a.brake, b.brake, pos, len),
            clutch: ramp_axis(a.clutch, b.clutch, pos, len),
            handbrake: ramp_axis(a.handbrake, b.handbrake, pos, len),
            // Buttons and gear selection are events, not levels.
            gear_request: b.gear_request,
            buttons: b.buttons,
        }
    }

    /// Reads the device every `device_divider` ticks and ramps the angle in
    /// between, so its derivative is not a train of spikes.
    fn device_angle(&mut self) -> Option<f64> {
        let rotation = self.pipeline.config().device_rotation_rad;
        if self.device_phase == 0 {
            if let Some(device) = self.device.as_mut() {
                match device.read_steering() {
                    Ok(Some(axis)) => {
                        // Devices report positive to the right; the sim's
                        // steering is positive to the left.
                        let angle = -f64::from(axis) * 0.5 * rotation;
                        self.angle_from = if self.angle_valid {
                            self.angle_to
                        } else {
                            angle
                        };
                        self.angle_to = angle;
                        self.angle_valid = true;
                    }
                    Ok(None) => self.angle_valid = false,
                    Err(e) => {
                        self.last_error = Some(e.to_string());
                        self.angle_valid = false;
                    }
                }
            } else {
                self.angle_valid = false;
            }
        }
        if !self.angle_valid {
            return None;
        }
        let t = f64::from(self.device_phase + 1) / f64::from(self.device_divider);
        Some(self.angle_from + (self.angle_to - self.angle_from) * t)
    }

    fn tick(&mut self) {
        let mut frame = self.next_frame();
        let angle = self.device_angle();
        if self.direct_steering {
            if let Some(a) = angle {
                let lock_angle = self.session.car().params().steering.max_wheel_angle_rad;
                let steer = (a / lock_angle).clamp(-1.0, 1.0);
                frame.steer = (steer * f64::from(AXIS_MAX)).round() as i16;
            }
        }
        self.session.step(&frame);

        let input = ffb_input(&self.session, angle);
        let sample = self.pipeline.step(&input);
        self.last_sample = sample;
        let rate = f64::from(self.session.tick_rate_hz());
        let clipping = if sample.clipped > CLIP_THRESHOLD {
            1.0
        } else {
            0.0
        };
        self.clip_fraction += (clipping - self.clip_fraction) / rate;
        self.peak_now = self.peak_now.max(sample.requested.abs());
        self.peak_ticks += 1;
        if self.peak_ticks >= self.session.tick_rate_hz() {
            self.peak_before = self.peak_now;
            self.peak_now = 0.0;
            self.peak_ticks = 0;
        }
        if let Some(log) = self.ffb_log.as_mut() {
            log.observe(
                &self.session,
                &frame,
                &input,
                &sample,
                self.pipeline.config(),
            );
        }

        if self.device_phase == 0 && self.ffb_enabled {
            if let Some(device) = self.device.as_mut() {
                let command = sample.device_command(self.pipeline.config());
                if let Err(e) = device.set_torque(command) {
                    // A device that fails once is dropped rather than retried
                    // a thousand times a second.
                    self.last_error = Some(e.to_string());
                    self.close_device();
                }
            }
        }
        self.device_phase = (self.device_phase + 1) % self.device_divider;

        if let Some(writer) = self.telemetry.as_mut() {
            telemetry::fill_row(&self.session, &frame, &sample, &mut self.row);
            if let Err(e) = writer.sample(&self.row) {
                self.last_error = Some(format!("telemetry: {e}"));
                self.telemetry = None;
            }
        }
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.record(frame, &self.session);
        }
        self.previous = self.current;
        self.current = RenderPose::of(&self.session);
        if self.ghost.observe(&self.session, &self.current) {
            self.save_ghost();
        }
    }

    /// Writes the best lap to the ghost file, if one was asked for.
    fn save_ghost(&mut self) {
        let (Some(path), Some(lap)) = (self.options.ghost_path.as_ref(), self.ghost.best()) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(path, lap.to_bytes()) {
            self.last_error = Some(format!("ghost {}: {e}", path.display()));
        }
    }

    fn run(mut self) -> Session {
        let rate = self.session.tick_rate_hz();
        let mut driver = FixedStepDriver::new(rate, self.options.max_catch_up_ticks);
        let mut last = Instant::now();
        let mut window_start = last;
        let mut window_ticks = 0u32;
        let mut worst_wake = 0u32;
        let mut measured_rate = 0.0;
        let mut reported_worst = 0u32;
        while !self.shared.stop.load(Ordering::SeqCst) {
            let commands: Vec<HostCommand> = std::mem::take(&mut *lock(&self.shared.commands));
            for command in commands {
                self.handle(command);
            }
            let now = Instant::now();
            let elapsed = now.duration_since(last).as_secs_f64();
            last = now;
            let ticks = if self.paused {
                0
            } else {
                driver.advance(elapsed)
            };
            for _ in 0..ticks {
                self.tick();
            }
            window_ticks += ticks;
            worst_wake = worst_wake.max(ticks);
            let window = now.duration_since(window_start).as_secs_f64();
            if window >= 1.0 {
                measured_rate = f64::from(window_ticks) / window;
                reported_worst = worst_wake;
                window_start = now;
                window_ticks = 0;
                worst_wake = 0;
            }
            {
                let mut p = lock(&self.shared.published);
                p.previous = self.previous;
                p.current = self.current;
                p.alpha = if self.paused { 1.0 } else { driver.alpha() };
                p.hud = HudState::of(&self.session);
                p.cones_down.clear();
                p.cones_down
                    .extend_from_slice(&self.session.timing().cone_down);
                p.ghost = self.ghost.pose_now(&self.session, p.alpha);
                p.ghost_time_s = self.ghost.best().map_or(0.0, GhostLap::seconds);
                p.ghost_delta_s = self.ghost.delta_s();
            }
            {
                let mut s = lock(&self.shared.status);
                s.device = self.device.as_ref().map(|d| d.info().clone());
                s.last_error = self.last_error.clone();
                s.ffb_enabled = self.ffb_enabled;
                s.ffb = self.last_sample;
                s.clip_fraction = self.clip_fraction;
                s.peak_requested = self.peak_now.max(self.peak_before);
                if let Some(log) = self.ffb_log.as_mut() {
                    self.ffb_log_status = log.status();
                    if log.is_done() {
                        self.ffb_log = None;
                    }
                }
                s.ffb_log = self.ffb_log_status.clone();
                s.measured_tick_rate_hz = measured_rate;
                s.worst_wake_ticks = reported_worst;
                s.dropped_time_s = driver.dropped_time();
                s.telemetry_rows = self.telemetry.as_ref().map(|t| t.rows());
                s.replay_frames = self.recorder.as_ref().map(|r| r.len());
                s.paused = self.paused;
                s.device_angle_rad = if self.angle_valid {
                    Some(self.angle_to)
                } else {
                    None
                };
            }
            std::thread::sleep(IDLE_SLEEP);
        }
        // Commands sent just before stopping, such as writing a replay, are
        // still carried out.
        let commands: Vec<HostCommand> = std::mem::take(&mut *lock(&self.shared.commands));
        for command in commands {
            self.handle(command);
        }
        self.close_device();
        self.close_telemetry();
        self.session
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pose_blend_takes_the_short_way_round() {
        let mut a = RenderPose::default();
        let mut b = RenderPose::default();
        a.pos = [0.0, 0.0, 0.0];
        b.pos = [2.0, 4.0, 6.0];
        // The same orientation written with opposite signs must not blend
        // through zero.
        a.rot = [0.6, 0.0, 0.0, 0.8];
        b.rot = [-0.6, 0.0, 0.0, -0.8];
        a.wheels[0].spin_angle = 3.0;
        b.wheels[0].spin_angle = -3.0; // wrapped past π
        let mid = RenderPose::lerp(&a, &b, 0.5);
        assert_eq!(mid.pos, [1.0, 2.0, 3.0]);
        let norm: f64 = mid.rot.iter().map(|r| r * r).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-12);
        assert!((mid.rot[0].abs() - 0.6).abs() < 1e-12);
        let expected = 3.0 + 0.5 * (2.0 * std::f64::consts::PI - 6.0);
        assert!((mid.wheels[0].spin_angle - expected).abs() < 1e-12);
        assert_eq!(RenderPose::lerp(&a, &b, 0.0).pos, a.pos);
        assert_eq!(RenderPose::lerp(&a, &b, 7.0).pos, b.pos);
    }

    #[test]
    fn axis_ramp_is_linear_and_ends_on_target() {
        assert_eq!(ramp_axis(0, 1000, 0, 10), 0);
        assert_eq!(ramp_axis(0, 1000, 5, 10), 500);
        assert_eq!(ramp_axis(0, 1000, 10, 10), 1000);
        assert_eq!(ramp_axis(0, 1000, 99, 10), 1000);
        assert_eq!(ramp_axis(-200, 200, 1, 4), -100);
    }
}
