// SPDX-License-Identifier: Apache-2.0
//! Driver input read on the physics thread.
//!
//! Wheel, pedals, shifter and handbrake can be read straight from their
//! devices once per physics tick instead of arriving through the front
//! end's per-frame input. A [`DirectInputMap`] says which device axis or
//! button drives which control and how it is calibrated; [`DirectInputs`]
//! reads the devices and writes the mapped controls into the tick's
//! [`InputFrame`], leaving everything unmapped as the front end sent it
//! (keyboard and gamepad keep working).
//!
//! The frame that reaches the physics is what replays record, so recordings
//! reproduce exactly whichever way the input arrived.
//!
//! While a control is mapped both ways, the value the front end sent is
//! compared with the one read here to measure how much later the per-frame
//! path delivers it: see [`LagMeter`].

use qts_core::input::{buttons, InputFrame, AXIS_MAX};
use qts_ffb::{InputSource, RawInputState, INPUT_AXES, INPUT_BUTTONS};

/// A control driven by an axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisAction {
    /// Steering, centred.
    Steer,
    /// Accelerator pedal.
    Throttle,
    /// Brake pedal.
    Brake,
    /// Clutch pedal.
    Clutch,
    /// Handbrake lever.
    Handbrake,
}

impl AxisAction {
    /// Parses the name the front end uses.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "steer" => Self::Steer,
            "throttle" => Self::Throttle,
            "brake" => Self::Brake,
            "clutch" => Self::Clutch,
            "handbrake" => Self::Handbrake,
            _ => return None,
        })
    }
}

/// A control driven by a button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonAction {
    /// Sequential shift up.
    ShiftUp,
    /// Sequential shift down.
    ShiftDown,
    /// Put the car back at the start.
    Reset,
    /// Handbrake fully on while held.
    Handbrake,
    /// Starter motor while held.
    Starter,
    /// One gate of an H-pattern shifter: the gear selected while held
    /// (−1 reverse, 1 and up forward).
    Gear(i8),
}

impl ButtonAction {
    /// Parses the name the front end uses (`gear_r`, `gear_1` … `gear_7`
    /// for shifter gates).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "shift_up" => Self::ShiftUp,
            "shift_down" => Self::ShiftDown,
            "reset" => Self::Reset,
            "handbrake_button" => Self::Handbrake,
            "starter" => Self::Starter,
            "gear_r" => Self::Gear(-1),
            other => {
                let n: i8 = other.strip_prefix("gear_")?.parse().ok()?;
                if (1..=7).contains(&n) {
                    Self::Gear(n)
                } else {
                    return None;
                }
            }
        })
    }
}

/// One axis mapping with its calibration, in the device's raw units
/// (−1 to 1).
#[derive(Clone, Debug, PartialEq)]
pub struct AxisBinding {
    /// Control driven.
    pub action: AxisAction,
    /// Device identifier.
    pub device: String,
    /// Axis index on the device.
    pub axis: usize,
    /// Raw value at one end of the travel.
    pub min: f32,
    /// Raw value at rest (the centre for steering).
    pub center: f32,
    /// Raw value at the other end.
    pub max: f32,
    /// Reverse the direction.
    pub invert: bool,
    /// Pedal shaping: deadzones and curve. Ignored for steering.
    pub shape: PedalShape,
}

/// Deadzones and response curve of a pedal or lever, applied after the
/// range calibration (D-085).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PedalShape {
    /// Share of travel at the released end that reads as zero.
    pub deadzone_low: f32,
    /// Share of travel at the pressed end that reads as full.
    pub deadzone_high: f32,
    /// Exponent of the response: 1 linear; above 1 gentler at first and
    /// steeper toward the end, as most drivers want a brake.
    pub curve: f32,
}

impl Default for PedalShape {
    fn default() -> Self {
        Self {
            deadzone_low: 0.0,
            deadzone_high: 0.0,
            curve: 1.0,
        }
    }
}

impl PedalShape {
    /// Shapes a calibrated travel, 0 to 1.
    pub fn apply(&self, travel: f32) -> f32 {
        let lo = self.deadzone_low.clamp(0.0, 0.45);
        let hi = self.deadzone_high.clamp(0.0, 0.45);
        let x = ((travel - lo) / (1.0 - lo - hi).max(0.05)).clamp(0.0, 1.0);
        let curve = self.curve.clamp(0.2, 5.0);
        if curve == 1.0 || x == 0.0 {
            return x;
        }
        qts_core::math::fm::powf(f64::from(x), f64::from(curve)) as f32
    }
}

impl AxisBinding {
    /// The calibrated value: −1 to 1 for steering (positive to the
    /// device's right), 0 to 1 for a pedal or lever. Matches the front
    /// end's calibration exactly.
    pub fn calibrate(&self, raw: f32) -> f32 {
        if self.action == AxisAction::Steer {
            let v = if raw >= self.center {
                (raw - self.center) / (self.max - self.center).max(0.001)
            } else {
                (raw - self.center) / (self.center - self.min).max(0.001)
            };
            let v = v.clamp(-1.0, 1.0);
            if self.invert {
                -v
            } else {
                v
            }
        } else {
            let v = ((raw - self.min) / (self.max - self.min).max(0.001)).clamp(0.0, 1.0);
            self.shape.apply(if self.invert { 1.0 - v } else { v })
        }
    }
}

/// One button mapping.
#[derive(Clone, Debug, PartialEq)]
pub struct ButtonBinding {
    /// Control driven.
    pub action: ButtonAction,
    /// Device identifier.
    pub device: String,
    /// Button index on the device.
    pub button: usize,
}

/// Every control read on the physics thread.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DirectInputMap {
    /// Axis mappings.
    pub axes: Vec<AxisBinding>,
    /// Button mappings.
    pub buttons: Vec<ButtonBinding>,
}

impl DirectInputMap {
    /// Device identifiers used, each once, in first-use order.
    pub fn devices(&self) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        let used = self
            .axes
            .iter()
            .map(|a| &a.device)
            .chain(self.buttons.iter().map(|b| &b.device));
        for id in used {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids
    }

    /// True if an H-pattern gate is mapped.
    pub fn has_shifter(&self) -> bool {
        self.buttons
            .iter()
            .any(|b| matches!(b.action, ButtonAction::Gear(_)))
    }
}

/// Measures how many ticks one signal trails another by matching their
/// shapes: the lag that minimises the mean absolute difference between the
/// late signal and the early one shifted.
#[derive(Clone, Debug)]
pub struct LagMeter {
    early: Vec<f32>,
    late: Vec<f32>,
    filled: usize,
    head: usize,
    since_estimate: usize,
    /// Latest estimate, ticks, and how many estimates it is the median of.
    estimates: Vec<u32>,
}

/// Ticks of history the lag is matched over.
pub const LAG_WINDOW: usize = 2000;
/// Largest lag searched for, ticks.
pub const LAG_MAX: usize = 150;
/// Smallest movement over the window (peak to peak, in −1 to 1 units) for
/// an estimate; a wheel held still says nothing about lag.
const LAG_MIN_SWING: f32 = 0.05;

impl Default for LagMeter {
    fn default() -> Self {
        Self {
            early: vec![0.0; LAG_WINDOW],
            late: vec![0.0; LAG_WINDOW],
            filled: 0,
            head: 0,
            since_estimate: 0,
            estimates: Vec::new(),
        }
    }
}

impl LagMeter {
    /// Adds one tick of both signals.
    pub fn push(&mut self, early: f32, late: f32) {
        self.early[self.head] = early;
        self.late[self.head] = late;
        self.head = (self.head + 1) % LAG_WINDOW;
        self.filled = (self.filled + 1).min(LAG_WINDOW);
        self.since_estimate += 1;
        if self.filled == LAG_WINDOW && self.since_estimate >= LAG_WINDOW / 2 {
            self.since_estimate = 0;
            if let Some(lag) = self.estimate() {
                self.estimates.push(lag);
                if self.estimates.len() > 31 {
                    self.estimates.remove(0);
                }
            }
        }
    }

    fn at(buffer: &[f32], head: usize, age: usize) -> f32 {
        // `age` 0 is the newest sample.
        buffer[(head + LAG_WINDOW - 1 - age) % LAG_WINDOW]
    }

    fn estimate(&self) -> Option<u32> {
        let (lo, hi) = self
            .early
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        if hi - lo < LAG_MIN_SWING {
            return None;
        }
        let span = LAG_WINDOW - LAG_MAX;
        let mut best = (f32::MAX, 0usize);
        for lag in 0..=LAG_MAX {
            let mut error = 0.0;
            for age in 0..span {
                error += (Self::at(&self.late, self.head, age)
                    - Self::at(&self.early, self.head, age + lag))
                .abs();
            }
            if error < best.0 {
                best = (error, lag);
            }
        }
        Some(best.1 as u32)
    }

    /// Median of the recent estimates, ticks, and how many there are.
    pub fn lag_ticks(&self) -> Option<(u32, usize)> {
        if self.estimates.is_empty() {
            return None;
        }
        let mut sorted = self.estimates.clone();
        sorted.sort_unstable();
        Some((sorted[sorted.len() / 2], sorted.len()))
    }
}

/// Devices open for reading and the map that uses them.
pub struct DirectInputs {
    map: DirectInputMap,
    devices: Vec<Box<dyn InputSource>>,
    states: Vec<RawInputState>,
    /// Index into `devices` for each axis and button binding.
    axis_device: Vec<usize>,
    button_device: Vec<usize>,
    last_error: Option<String>,
    steer_lag: LagMeter,
    compare_steer: bool,
}

/// What [`DirectInputs::apply`] did, for diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DirectReport {
    /// True if every device was read.
    pub ok: bool,
    /// Steering read from the device this tick, −1 to 1 positive left, if
    /// it is mapped.
    pub steer: Option<f32>,
}

impl DirectInputs {
    /// Opens the devices the map uses with `open`. A device that cannot be
    /// opened is reported and its bindings dropped, so the rest still work.
    pub fn open(
        map: DirectInputMap,
        mut open: impl FnMut(&str) -> Result<Box<dyn InputSource>, String>,
    ) -> (Self, Vec<String>) {
        let mut errors = Vec::new();
        let mut devices = Vec::new();
        let mut opened: Vec<String> = Vec::new();
        for id in map.devices() {
            match open(&id) {
                Ok(device) => {
                    devices.push(device);
                    opened.push(id);
                }
                Err(e) => errors.push(format!("input device {id}: {e}")),
            }
        }
        let mut kept = map.clone();
        kept.axes
            .retain(|a| opened.contains(&a.device) && a.axis < INPUT_AXES);
        kept.buttons
            .retain(|b| opened.contains(&b.device) && b.button < INPUT_BUTTONS);
        let index = |id: &String| opened.iter().position(|o| o == id).unwrap_or(0);
        let axis_device = kept.axes.iter().map(|a| index(&a.device)).collect();
        let button_device = kept.buttons.iter().map(|b| index(&b.device)).collect();
        let states = vec![RawInputState::default(); devices.len()];
        (
            Self {
                map: kept,
                devices,
                states,
                axis_device,
                button_device,
                last_error: None,
                steer_lag: LagMeter::default(),
                compare_steer: false,
            },
            errors,
        )
    }

    /// The map in use, after dropping bindings to devices that would not
    /// open.
    pub fn map(&self) -> &DirectInputMap {
        &self.map
    }

    /// Compare the front end's steering with the device's to measure the
    /// delay of the per-frame path. Only meaningful when the front end
    /// reads the same wheel.
    pub fn set_compare_steering(&mut self, on: bool) {
        self.compare_steer = on;
    }

    /// The last read error, if any.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Measured delay of the front end's steering behind the device's,
    /// ticks, with the number of estimates behind it.
    pub fn steering_lag_ticks(&self) -> Option<(u32, usize)> {
        self.steer_lag.lag_ticks()
    }

    /// Reads every device once and writes the mapped controls into
    /// `frame`. Unmapped controls are left as they are. A device that
    /// fails to read keeps its last state for this tick.
    pub fn apply(&mut self, frame: &mut InputFrame) -> DirectReport {
        let mut ok = true;
        for (device, state) in self.devices.iter_mut().zip(self.states.iter_mut()) {
            match device.read_state() {
                Ok(s) => *state = s,
                Err(e) => {
                    ok = false;
                    self.last_error = Some(e.to_string());
                }
            }
        }
        let from_frontend = frame.steer_f64() as f32;
        let mut steer = None;
        let full = f32::from(AXIS_MAX);
        for (binding, &d) in self.map.axes.iter().zip(&self.axis_device) {
            let value = binding.calibrate(self.states[d].axes[binding.axis]);
            match binding.action {
                AxisAction::Steer => {
                    // Devices report positive to the right; the simulation's
                    // steering is positive to the left.
                    frame.steer = (-value * full).round() as i16;
                    steer = Some(-value);
                }
                AxisAction::Throttle => frame.throttle = (value * full).round() as i16,
                AxisAction::Brake => frame.brake = (value * full).round() as i16,
                AxisAction::Clutch => frame.clutch = (value * full).round() as i16,
                AxisAction::Handbrake => frame.handbrake = (value * full).round() as i16,
            }
        }
        let mut gear = None;
        for (binding, &d) in self.map.buttons.iter().zip(&self.button_device) {
            if !self.states[d].button(binding.button) {
                continue;
            }
            match binding.action {
                ButtonAction::ShiftUp => frame.buttons |= buttons::SHIFT_UP,
                ButtonAction::ShiftDown => frame.buttons |= buttons::SHIFT_DOWN,
                ButtonAction::Reset => frame.buttons |= buttons::RESET,
                ButtonAction::Handbrake => frame.handbrake = AXIS_MAX,
                ButtonAction::Starter => frame.buttons |= buttons::STARTER,
                ButtonAction::Gear(g) => gear = Some(g),
            }
        }
        if self.map.has_shifter() {
            // An H-pattern shifter always says where the lever is: in a
            // gate, or in neutral between them.
            frame.gear_request = gear.unwrap_or(0);
        }
        if self.compare_steer {
            if let Some(s) = steer {
                self.steer_lag.push(s, from_frontend);
            }
        }
        DirectReport { ok, steer }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pedal_deadzones_and_curve_shape_the_travel() {
        let flat = PedalShape::default();
        for t in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(flat.apply(t), t);
        }
        let shaped = PedalShape {
            deadzone_low: 0.05,
            deadzone_high: 0.05,
            curve: 2.0,
        };
        // Inside the deadzones: zero at rest, full at the floor.
        assert_eq!(shaped.apply(0.04), 0.0);
        assert_eq!(shaped.apply(0.96), 1.0);
        // Half way through the live travel, squared.
        assert!((shaped.apply(0.5) - 0.25).abs() < 1e-6);
        // Monotonic and continuous.
        let mut prev = 0.0;
        for i in 0..=1000 {
            let v = shaped.apply(i as f32 / 1000.0);
            assert!(v >= prev && v - prev < 0.01);
            prev = v;
        }
        // A brake binding shapes; steering ignores the shape.
        let brake = AxisBinding {
            action: AxisAction::Brake,
            device: "p".into(),
            axis: 0,
            min: -1.0,
            center: -1.0,
            max: 1.0,
            invert: false,
            shape: shaped,
        };
        assert!((brake.calibrate(0.0) - 0.25).abs() < 1e-6);
        let steer = AxisBinding {
            action: AxisAction::Steer,
            center: 0.0,
            ..brake.clone()
        };
        assert!((steer.calibrate(0.5) - 0.5).abs() < 1e-6);
    }
    use qts_ffb::ScriptedInput;

    fn wheel_map() -> DirectInputMap {
        DirectInputMap {
            axes: vec![
                AxisBinding {
                    action: AxisAction::Steer,
                    device: "wheel".into(),
                    axis: 0,
                    min: -1.0,
                    center: 0.0,
                    max: 1.0,
                    invert: false,
                    shape: PedalShape::default(),
                },
                AxisBinding {
                    action: AxisAction::Brake,
                    device: "wheel".into(),
                    axis: 2,
                    min: -1.0,
                    center: -1.0,
                    max: 1.0,
                    invert: true,
                    shape: PedalShape::default(),
                },
            ],
            buttons: vec![ButtonBinding {
                action: ButtonAction::ShiftUp,
                device: "wheel".into(),
                button: 5,
            }],
        }
    }

    #[test]
    fn mapped_controls_replace_the_frame_and_the_rest_are_kept() {
        let (mut inputs, errors) = DirectInputs::open(wheel_map(), |id| {
            let mut d = ScriptedInput::new(id);
            d.state.axes[0] = 0.5; // half right
            d.state.axes[2] = -1.0; // inverted brake: fully pressed
            d.state.buttons = 1 << 5;
            Ok(Box::new(d) as Box<dyn InputSource>)
        });
        assert!(errors.is_empty());
        let mut frame = InputFrame::from_analog(0.3, 0.7, 0.0, 0.0, 0.0);
        let report = inputs.apply(&mut frame);
        assert!(report.ok);
        assert_eq!(frame.steer, -(AXIS_MAX / 2) - 1, "half right is negative");
        assert_eq!(frame.brake, AXIS_MAX);
        assert_eq!(
            frame.throttle,
            InputFrame::from_analog(0.0, 0.7, 0.0, 0.0, 0.0).throttle
        );
        assert!(frame.pressed(buttons::SHIFT_UP));
        assert_eq!(frame.gear_request, qts_core::input::GEAR_NO_REQUEST);
    }

    #[test]
    fn a_device_that_will_not_open_drops_only_its_bindings() {
        let mut map = wheel_map();
        map.axes.push(AxisBinding {
            action: AxisAction::Throttle,
            device: "pedals".into(),
            axis: 1,
            min: -1.0,
            center: -1.0,
            max: 1.0,
            invert: false,
            shape: PedalShape::default(),
        });
        let (inputs, errors) = DirectInputs::open(map, |id| {
            if id == "pedals" {
                Err("unplugged".into())
            } else {
                Ok(Box::new(ScriptedInput::new(id)) as Box<dyn InputSource>)
            }
        });
        assert_eq!(errors.len(), 1);
        assert_eq!(inputs.map().axes.len(), 2);
    }

    #[test]
    fn shifter_gates_select_gears_and_neutral_between_them() {
        let map = DirectInputMap {
            axes: vec![],
            buttons: vec![
                ButtonBinding {
                    action: ButtonAction::Gear(1),
                    device: "shifter".into(),
                    button: 0,
                },
                ButtonBinding {
                    action: ButtonAction::Gear(-1),
                    device: "shifter".into(),
                    button: 6,
                },
            ],
        };
        let (mut inputs, _) = DirectInputs::open(map, |id| {
            let mut d = ScriptedInput::new(id);
            d.state.buttons = 1 << 6;
            Ok(Box::new(d) as Box<dyn InputSource>)
        });
        let mut frame = InputFrame::default();
        inputs.apply(&mut frame);
        assert_eq!(frame.gear_request, -1);
        assert_eq!(
            ButtonAction::from_name("gear_3"),
            Some(ButtonAction::Gear(3))
        );
        assert_eq!(ButtonAction::from_name("gear_9"), None);
        assert_eq!(
            ButtonAction::from_name("gear_r"),
            Some(ButtonAction::Gear(-1))
        );
    }

    #[test]
    fn calibration_matches_the_front_end() {
        let steer = AxisBinding {
            action: AxisAction::Steer,
            device: String::new(),
            axis: 0,
            min: -0.8,
            center: 0.1,
            max: 0.9,
            invert: false,
            shape: PedalShape::default(),
        };
        assert_eq!(steer.calibrate(0.1), 0.0);
        assert!((steer.calibrate(0.5) - 0.5).abs() < 1e-6);
        assert!((steer.calibrate(-0.35) + 0.5).abs() < 1e-6);
        assert_eq!(steer.calibrate(2.0), 1.0);
        let pedal = AxisBinding {
            action: AxisAction::Throttle,
            min: -1.0,
            center: -1.0,
            max: 0.6,
            ..steer
        };
        assert_eq!(pedal.calibrate(-1.0), 0.0);
        assert!((pedal.calibrate(-0.2) - 0.5).abs() < 1e-6);
        assert_eq!(pedal.calibrate(0.9), 1.0);
    }

    #[test]
    fn the_lag_meter_finds_a_known_delay() {
        let mut meter = LagMeter::default();
        let delay = 23;
        let signal = |t: usize| ((t as f32) * 0.004).sin() * 0.5 + ((t as f32) * 0.017).sin() * 0.1;
        for t in 0..6000 {
            let late = if t >= delay { signal(t - delay) } else { 0.0 };
            meter.push(signal(t), late);
        }
        let (lag, n) = meter.lag_ticks().unwrap();
        assert_eq!(lag, delay as u32);
        assert!(n >= 3);
        // A wheel held still gives no estimate.
        let mut still = LagMeter::default();
        for _ in 0..6000 {
            still.push(0.1, 0.1);
        }
        assert!(still.lag_ticks().is_none());
    }
}
