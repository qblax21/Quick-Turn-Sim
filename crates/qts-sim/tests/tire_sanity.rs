// SPDX-License-Identifier: Apache-2.0
//! Tire sanity against published ranges (D-086), on the same curves the
//! plots in docs/img are drawn from. Physics is not changed to pass these:
//! a tire outside a range is listed below as a known gap, with the reason,
//! and the test fails if the gap closes so the list stays true.

mod common;

use common::*;
use qts_sim::tire_plots::{curves, peak};

/// Production-car tires develop their peak lateral force at 6 to 10
/// degrees of slip angle, more at higher load: Milliken and Milliken, Race
/// Car Vehicle Dynamics (SAE, 1995) p. 27, as given in Wikipedia, "Tire load
/// sensitivity" (docs/VALIDATION_SOURCES.md).
const PEAK_SLIP_ANGLE_DEG: (f64, f64) = (6.0, 10.0);
/// Peak lateral force grows roughly as the load to a power of 0.7 to 0.9
/// (same article; marked there as needing a citation).
const LOAD_EXPONENT: (f64, f64) = (0.7, 0.9);
/// Tires outside a published range at their reference load, and why.
const KNOWN_GAPS: [(&str, &str); 1] = [(
    "street_205_45r17",
    "peak lateral slip angle 10.15 degrees, 0.15 beyond the published range; no car uses this tire since D-078; raising its cornering stiffness (pky1) would bring the peak in, a physics change awaiting approval",
)];

/// Combined braking and cornering should stay inside the friction ellipse
/// of the pure peaks. All three placeholder files go up to about 4 % past
/// it, braking at about 6 degrees of slip angle; most likely their
/// combined-slip weighting coefficients (rbx1, rbx2, which set how fast
/// longitudinal force falls with slip angle), not yet confirmed. Raising them is a physics change awaiting
/// approval; the test fails if the excess grows past this or disappears.
const ELLIPSE_GAP: (f64, &str) = (
    1.05,
    "placeholder combined-slip weights; awaiting approval to change",
);

#[test]
fn every_tire_file_is_inside_the_published_ranges() {
    let root = data();
    let surface = root.load_surface("tarmac").unwrap().value;
    for id in root.list("tires").unwrap() {
        let p = root.load_tire(&id).unwrap().value;
        let c = curves(&p, &surface);
        let peaks: Vec<(f64, f64)> = c.lateral.iter().map(|l| peak(l)).collect();
        // Peak slip angle at the reference load.
        let at_ref = peaks[1].0;
        let inside = (PEAK_SLIP_ANGLE_DEG.0..=PEAK_SLIP_ANGLE_DEG.1).contains(&at_ref);
        let gap = KNOWN_GAPS.iter().find(|(t, _)| *t == id);
        match gap {
            None => assert!(
                inside,
                "{id}: lateral peak at {at_ref:.2} deg, published {PEAK_SLIP_ANGLE_DEG:?}"
            ),
            Some((_, why)) => {
                assert!(
                    !inside,
                    "{id}: the known gap has closed ({at_ref:.2} deg): remove it"
                );
                println!("{id}: known gap: {why}");
            }
        }
        // The peak moves to larger angles as load rises.
        assert!(
            peaks[0].0 < peaks[1].0 && peaks[1].0 < peaks[2].0,
            "{id}: {peaks:?}"
        );
        // Load sensitivity: the friction coefficient falls with load...
        let mu: Vec<f64> = peaks.iter().zip(c.loads).map(|(p, fz)| p.1 / fz).collect();
        assert!(mu[0] > mu[1] && mu[1] > mu[2], "{id}: peak Fy/Fz {mu:?}");
        // ...while the peak force still rises, as a power of the load.
        let e = (peaks[2].1 / peaks[0].1).ln() / (c.loads[2] / c.loads[0]).ln();
        assert!(
            (LOAD_EXPONENT.0..=LOAD_EXPONENT.1).contains(&e),
            "{id}: peak force goes as load^{e:.2}"
        );
        // Longitudinal: a peak, then a fall into sliding (no published
        // range found for the slip ratio of the peak; plotted, not judged).
        for l in &c.longitudinal {
            let (s, f) = peak(l);
            assert!(
                s > 0.0 && s < 0.4 && l.last().unwrap().1 < f,
                "{id}: no longitudinal peak"
            );
        }
        // Combined slip stays inside the friction ellipse of the pure
        // peaks at the reference load (within 3 %): the pure longitudinal
        // peak in the same direction (drive or braking) and the pure
        // lateral peak.
        let fz = c.loads[1];
        let pure_x = |sign: f64| {
            (1..=400)
                .map(|i| {
                    let op = qts_tire::magic::Operating {
                        fz,
                        kappa: sign * f64::from(i) * 0.001,
                        tan_alpha: 0.0,
                        camber: 0.0,
                        dpi: 0.0,
                        grip_factor: 1.0,
                        stiffness_factor: 1.0,
                    };
                    qts_tire::magic::steady_state(&p, &surface, &op).fx.abs()
                })
                .fold(0.0_f64, f64::max)
        };
        let (drive_max, brake_max, fy_max) = (pure_x(1.0), pure_x(-1.0), peaks[1].1);
        let mut worst = (0.0_f64, 0.0, 0.0);
        for (alpha, pts) in &c.combined {
            for (fx, fy) in pts {
                let fx_max = if *fx >= 0.0 { drive_max } else { brake_max };
                let r = ((fx / fx_max).powi(2) + (fy / fy_max).powi(2)).sqrt();
                if r > worst.0 {
                    worst = (r, *alpha, *fx);
                }
            }
        }
        println!(
            "{id}: combined slip reaches {:.3} of the ellipse (at {} deg, Fx {:.0} N)",
            worst.0, worst.1, worst.2
        );
        // Known gap for every file: see ELLIPSE_GAP.
        assert!(
            worst.0 > 1.0 && worst.0 <= ELLIPSE_GAP.0,
            "{id}: combined slip at {:.3} of the ellipse; the recorded gap is up to {} ({})",
            worst.0,
            ELLIPSE_GAP.0,
            ELLIPSE_GAP.1
        );
        // The plot is drawn from these curves.
        let svg = qts_sim::tire_plots::svg(&c, &p.name);
        assert!(svg.starts_with("<svg") && svg.contains(&p.name));
    }
}
