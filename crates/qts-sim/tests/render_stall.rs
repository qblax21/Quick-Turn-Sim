// SPDX-License-Identifier: Apache-2.0
//! Physics and force feedback run at a fixed rate on the host thread,
//! apart from rendering. The renderer only hands over driver input, at its
//! frame rate. These tests script the timing with a [`SteppedHost`] (the
//! host's own per-tick code, driven by a test clock): the same drive is
//! run with input arriving at 240 frames per second, at 30, at 30 with
//! long render stalls, and with the host thread itself waking late. The
//! tick count must not depend on the frame rate, and the force sent to the
//! wheel must stay as smooth as it is at 240 fps.

mod common;

use common::*;
use qts_core::input::InputFrame;
use qts_ffb::FfbSample;
use qts_sim::host::{HostOptions, SteppedHost};

const SECONDS: f64 = 6.0;
/// The host thread wakes about every millisecond.
const WAKE_S: f64 = 0.001;

fn options() -> HostOptions {
    HostOptions {
        ffb: data().load_ffb("generic_wheel").unwrap().value,
        max_catch_up_ticks: 100,
        interpolate_inputs: true,
        ghost_path: None,
    }
}

/// The driver's hands: a weave at 0.5 Hz with some throttle, as a function
/// of the time the renderer samples them.
fn hands(t: f64) -> InputFrame {
    let steer = 0.12 * (std::f64::consts::TAU * 0.5 * t).sin();
    InputFrame::from_analog(steer, 0.35, 0.0, 0.0, 0.0)
}

struct Timing {
    /// Frames per second of the renderer.
    fps: f64,
    /// Render stalls: (start s, length s). No input is handed over during
    /// one, as when a frame takes that long.
    render_stalls: &'static [(f64, f64)],
    /// Host-thread stalls: (start s, length s). The thread wakes once at
    /// the end and catches up.
    host_stalls: &'static [(f64, f64)],
}

fn drive(car: &str, timing: &Timing) -> Vec<FfbSample> {
    let mut host = SteppedHost::new(session(car, "test_track"), options()).unwrap();
    let mut out = Vec::new();
    let mut clock = 0.0;
    let mut next_frame = 0.0;
    let in_stall =
        |t: f64, stalls: &[(f64, f64)]| stalls.iter().find(|(a, d)| t >= *a && t < a + d).copied();
    while clock < SECONDS - 1e-9 {
        // The renderer hands over input when a frame is due, unless it is
        // stuck in a long frame.
        if clock + 1e-9 >= next_frame {
            if let Some((a, d)) = in_stall(clock, timing.render_stalls) {
                next_frame = a + d;
            } else {
                host.set_input(hands(clock));
                next_frame += 1.0 / timing.fps;
            }
        }
        // The host thread wakes; if it is stalled, it wakes once at the end.
        let step = match in_stall(clock, timing.host_stalls) {
            Some((a, d)) if (clock - a).abs() < 1e-9 => d,
            _ => WAKE_S,
        };
        out.extend(host.advance(step));
        clock += step;
    }
    out
}

/// Largest change of the output to the wheel between consecutive ticks.
fn worst_step(samples: &[FfbSample]) -> f64 {
    samples
        .windows(2)
        .map(|w| (w[1].output - w[0].output).abs())
        .fold(0.0, f64::max)
}

/// Largest change of slope between consecutive ticks: a staircase in the
/// output (a jump at each frame boundary) shows up here even when each
/// step is small.
fn worst_kink(samples: &[FfbSample]) -> f64 {
    samples
        .windows(3)
        .map(|w| (w[2].output - 2.0 * w[1].output + w[0].output).abs())
        .fold(0.0, f64::max)
}

fn rms_difference(a: &[FfbSample], b: &[FfbSample]) -> f64 {
    let n = a.len().min(b.len());
    let sum: f64 = a[..n]
        .iter()
        .zip(&b[..n])
        .map(|(x, y)| (x.output - y.output).powi(2))
        .sum();
    (sum / n as f64).sqrt()
}

#[test]
fn the_tick_count_does_not_depend_on_the_frame_rate() {
    for fps in [240.0, 60.0, 30.0, 12.0] {
        let timing = Timing {
            fps,
            render_stalls: &[(2.0, 0.3)],
            host_stalls: &[],
        };
        let n = drive("hatch_tern_gt", &timing).len();
        assert!(
            (5999..=6001).contains(&n),
            "{fps} fps: {n} ticks in {SECONDS} s"
        );
    }
}

#[test]
fn force_feedback_stays_smooth_at_30_fps_and_through_render_stalls() {
    for car in ["hatch_tern_gt", "sedan_halden_e4"] {
        let reference = drive(
            car,
            &Timing {
                fps: 240.0,
                render_stalls: &[],
                host_stalls: &[],
            },
        );
        let at_30 = drive(
            car,
            &Timing {
                fps: 30.0,
                render_stalls: &[],
                host_stalls: &[],
            },
        );
        let stalled = drive(
            car,
            &Timing {
                fps: 30.0,
                // A quarter-second hitch, then two shorter ones.
                render_stalls: &[(1.5, 0.25), (3.0, 0.1), (4.5, 0.15)],
                host_stalls: &[],
            },
        );
        let late_host = drive(
            car,
            &Timing {
                fps: 30.0,
                render_stalls: &[],
                // The host thread scheduled 40 ms late once: it catches up
                // with 40 ticks in one wake-up.
                host_stalls: &[(2.5, 0.040)],
            },
        );
        let ref_step = worst_step(&reference);
        let ref_kink = worst_kink(&reference);
        let summary = format!(
            "{car}: worst output step / kink per tick: 240 fps {ref_step:.4} / {ref_kink:.4}, \
             30 fps {:.4} / {:.4}, 30 fps with stalls {:.4} / {:.4}, late host {:.4} / {:.4}; \
             rms difference from 240 fps at 30 fps {:.4}",
            worst_step(&at_30),
            worst_kink(&at_30),
            worst_step(&stalled),
            worst_kink(&stalled),
            worst_step(&late_host),
            worst_kink(&late_host),
            rms_difference(&reference, &at_30)
        );
        eprintln!("{summary}");
        // At 30 fps, and after a late wake-up of the host thread, the force
        // must be as smooth as at 240 fps: no staircase at frame
        // boundaries, no burst from catching up.
        for (label, s) in [("30 fps", &at_30), ("late host", &late_host)] {
            let (w, k) = (worst_step(s), worst_kink(s));
            assert!(
                w <= 1.05 * ref_step + 1e-4,
                "{summary}\n{label}: step {w:.4}"
            );
            assert!(
                k <= 1.05 * ref_kink + 1e-4,
                "{summary}\n{label}: kink {k:.4}"
            );
        }
        // After a render stall the input jumps to where the driver's hands
        // now are, and the host spreads that over at most 50 ms, so the car
        // really does steer faster for a moment and the force changes
        // faster. It must still stay within twice the reference's worst
        // step: no single-tick spike.
        let w = worst_step(&stalled);
        assert!(w <= 2.0 * ref_step, "{summary}\nrender stalls: step {w:.4}");
        for (label, s) in [
            ("30 fps", &at_30),
            ("render stalls", &stalled),
            ("late host", &late_host),
        ] {
            assert!(
                s.iter()
                    .all(|x| x.output.is_finite() && x.output.abs() <= 1.0),
                "{label}"
            );
        }
    }
}
