// SPDX-License-Identifier: Apache-2.0
//! Tire sanity against published ranges (D-086), on the same curves the
//! plots in docs/img are drawn from. The two gaps D-086 recorded (combined
//! slip beyond the friction ellipse, the street tire's peak angle) were
//! closed in D-087, so every check here is a plain bound.

mod common;

use common::*;
use qts_sim::tire_plots::{curves, peak};
use qts_tire::magic::{steady_state, Operating};

/// Production-car tires develop their peak lateral force at 6 to 10
/// degrees of slip angle, more at higher load: Milliken and Milliken, Race
/// Car Vehicle Dynamics (SAE, 1995) p. 27, as given in Wikipedia, "Tire load
/// sensitivity" (docs/VALIDATION_SOURCES.md).
const PEAK_SLIP_ANGLE_DEG: (f64, f64) = (6.0, 10.0);
/// Peak lateral force grows roughly as the load to a power of 0.7 to 0.9
/// (same article; marked there as needing a citation).
const LOAD_EXPONENT: (f64, f64) = (0.7, 0.9);

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
        assert!(
            (PEAK_SLIP_ANGLE_DEG.0..=PEAK_SLIP_ANGLE_DEG.1).contains(&at_ref),
            "{id}: lateral peak at {at_ref:.2} deg, published {PEAK_SLIP_ANGLE_DEG:?}"
        );
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
        // peaks at the reference load: the pure longitudinal peak in the
        // same direction (drive or braking) and the pure lateral peak.
        let fz = c.loads[1];
        let pure_x = |sign: f64| {
            (1..=400)
                .map(|i| {
                    let op = Operating {
                        fz,
                        kappa: sign * f64::from(i) * 0.001,
                        tan_alpha: 0.0,
                        camber: 0.0,
                        dpi: 0.0,
                        grip_factor: 1.0,
                        stiffness_factor: 1.0,
                    };
                    steady_state(&p, &surface, &op).fx.abs()
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
            "{id}: lateral peak {at_ref:.2} deg; combined slip reaches {:.4} of the ellipse (at {} deg, Fx {:.0} N)",
            worst.0, worst.1, worst.2
        );
        assert!(
            worst.0 <= 1.0 + 1e-6,
            "{id}: combined slip at {:.4} of the ellipse (at {} deg, Fx {:.0} N)",
            worst.0,
            worst.1,
            worst.2
        );
        // The plot is drawn from these curves.
        let svg = qts_sim::tire_plots::svg(&c, &p.name);
        assert!(svg.starts_with("<svg") && svg.contains(&p.name));
    }
}
