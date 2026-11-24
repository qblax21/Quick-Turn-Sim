// SPDX-License-Identifier: Apache-2.0
//! The force-feedback gain helper (D-123): from one lap at a gain that
//! clips, it suggests a gain that keeps clipping under 2 percent of that
//! lap, checked by driving the same lap again at the suggested gain.

mod common;

use common::*;
use qts_core::step::Simulation;
use qts_sim::ai::{AiConfig, AiDriver};
use qts_sim::drivers::Driver;
use qts_sim::ffb_log::CLIP_THRESHOLD;
use qts_sim::host::{ffb_input, GAIN_TARGET_CLIP_PERCENT};
use qts_sim::lap_telemetry::GainHelper;
use qts_sim::session::Session;

/// One timed lap of Ridgeline by the computer driver in the Tern GT,
/// through the CSL Elite preset at `gain`: the clipped share of the lap,
/// percent, and the helper's suggestion from it.
fn lap_at(gain: f64) -> (f64, Option<f64>) {
    let root = data();
    let mut s = Session::from_data(
        &root,
        "hatch_tern_gt",
        "ridgeline_circuit",
        AiDriver::session_config(),
    )
    .unwrap();
    let mut ai = AiDriver::new(s.track(), AiConfig::grip(0.8)).unwrap();
    let mut config = root.load_ffb("fanatec_csl_elite").unwrap().value;
    config.gain = gain;
    let knee = config.soft_clip_knee;
    let mut pipeline = qts_ffb::FfbPipeline::new(config, s.tick_rate_hz()).unwrap();
    let mut helper = GainHelper::default();
    let (mut ticks, mut clipped) = (0u64, 0u64);
    loop {
        let f = ai.frame(&s);
        s.step(&f);
        let sample = pipeline.step(&ffb_input(&s, None));
        let t = s.timing();
        helper.observe((t.runs, t.invalid_runs), t.running, sample.requested);
        // The first timed lap only: from the line after the out lap.
        if t.runs + t.invalid_runs >= 1 {
            break;
        }
        if t.running {
            ticks += 1;
            if sample.clipped > CLIP_THRESHOLD {
                clipped += 1;
            }
        }
        assert!(s.tick() < 400_000, "no lap in 400 s");
    }
    let percent = 100.0 * clipped as f64 / ticks.max(1) as f64;
    let rate = f64::from(s.tick_rate_hz());
    (
        percent,
        helper.suggest(gain, knee, GAIN_TARGET_CLIP_PERCENT, rate),
    )
}

#[test]
fn the_suggested_gain_keeps_clipping_under_two_percent() {
    let (before, suggested) = lap_at(1.4);
    let suggested = suggested.expect("no suggestion after a full lap");
    let (after, _) = lap_at(suggested);
    eprintln!("gain 1.40: {before:.1} % clipped; suggested {suggested:.3}: {after:.2} % clipped");
    assert!(
        before > 5.0,
        "gain 1.4 clipped only {before:.1} %: not a test"
    );
    assert!(after < 2.0, "{after:.2} % clipped at the suggested gain");
    // And not far under it: the helper does not just turn the wheel down.
    assert!(
        after > 0.3,
        "{after:.2} % clipped: the suggestion is too timid"
    );
}
