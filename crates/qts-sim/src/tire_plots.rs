// SPDX-License-Identifier: Apache-2.0
//! Tire sanity curves (D-086): lateral force against slip angle and
//! longitudinal force against slip ratio at three loads, and the
//! combined-slip envelope (the friction ellipse) at the reference load,
//! from the steady-state Magic Formula as the simulation uses it. The same
//! curves feed the plots and the tests that check them against published
//! ranges, so a plot is exactly what was checked.

use qts_tire::magic::{steady_state, Operating};
use qts_tire::{SurfaceParams, TireParams};
use std::fmt::Write as _;

/// Loads the curves are drawn at, as shares of the tire's reference load.
pub const LOAD_SHARES: [f64; 3] = [0.5, 1.0, 1.5];

/// The curves of one tire.
#[derive(Clone, Debug)]
pub struct Curves {
    /// Tire identifier.
    pub id: String,
    /// The three loads, N.
    pub loads: [f64; 3],
    /// Per load: (slip angle in degrees, lateral force in N).
    pub lateral: Vec<Vec<(f64, f64)>>,
    /// Per load: (slip ratio, longitudinal force in N).
    pub longitudinal: Vec<Vec<(f64, f64)>>,
    /// Combined slip at the reference load: one curve per slip angle of
    /// (longitudinal force, lateral force) as the slip ratio sweeps from
    /// full braking to full drive.
    pub combined: Vec<(f64, Vec<(f64, f64)>)>,
}

fn op(fz: f64, kappa: f64, alpha_deg: f64) -> Operating {
    Operating {
        fz,
        kappa,
        tan_alpha: qts_core::math::fm::tan(alpha_deg.to_radians()),
        camber: 0.0,
        dpi: 0.0,
        grip_factor: 1.0,
        stiffness_factor: 1.0,
    }
}

/// Computes the curves of a tire on a surface, at its reference pressure
/// and optimum temperature.
pub fn curves(p: &TireParams, surface: &SurfaceParams) -> Curves {
    let fz0 = p.vertical.nominal_load_n;
    let loads = LOAD_SHARES.map(|k| k * fz0);
    let lateral = loads
        .iter()
        .map(|&fz| {
            (0..=300)
                .map(|i| {
                    let a = f64::from(i) * 0.05;
                    (a, steady_state(p, surface, &op(fz, 0.0, a)).fy.abs())
                })
                .collect()
        })
        .collect();
    let longitudinal = loads
        .iter()
        .map(|&fz| {
            (0..=400)
                .map(|i| {
                    let s = f64::from(i) * 0.001;
                    (s, steady_state(p, surface, &op(fz, s, 0.0)).fx)
                })
                .collect()
        })
        .collect();
    let combined = [1.0, 2.0, 4.0, 6.0, 9.0, 12.0]
        .iter()
        .map(|&a| {
            let pts = (-300..=300)
                .map(|i| {
                    let s = f64::from(i) * 0.001;
                    let f = steady_state(p, surface, &op(fz0, s, a));
                    (f.fx, f.fy.abs())
                })
                .collect();
            (a, pts)
        })
        .collect();
    Curves {
        id: p.id.clone(),
        loads,
        lateral,
        longitudinal,
        combined,
    }
}

/// The highest value of a curve and where it is: (x, y).
pub fn peak(curve: &[(f64, f64)]) -> (f64, f64) {
    curve.iter().copied().fold(
        (0.0, f64::MIN),
        |best, p| if p.1 > best.1 { p } else { best },
    )
}

/// One line of a chart: points, colour and width.
type Series<'a> = (Vec<(f64, f64)>, &'a str, f64);

/// The curves as an SVG chart: three panels side by side.
pub fn svg(c: &Curves, name: &str) -> String {
    let (w, h) = (1200.0, 470.0);
    let (pw, ph, top) = (330.0, 300.0, 90.0);
    let lefts = [70.0, 470.0, 860.0];
    let colours = ["#8a8f98", "#d9480f", "#1f6feb"];
    let mut s = String::new();
    let _ = write!(
        s,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="Segoe UI, Arial, sans-serif"><rect width="{w}" height="{h}" fill="#ffffff"/><text x="70" y="30" font-size="17" font-weight="700" fill="#111">{name}: steady-state tire forces (Magic Formula as simulated)</text><text x="70" y="52" font-size="12.5" fill="#555">Reference pressure, optimum temperature, tarmac. Loads {:.0}, {:.0} and {:.0} N (half, one and one and a half times the reference). Placeholder coefficients.</text>"##,
        c.loads[0], c.loads[1], c.loads[2]
    );
    let fmax = c.loads[2] * 1.4;
    let mut panel = |left: f64,
                     title: &str,
                     xlabel: &str,
                     xmin: f64,
                     xmax: f64,
                     ymin: f64,
                     ymax: f64,
                     series: &[Series<'_>]| {
        let x = |v: f64| left + (v - xmin) / (xmax - xmin) * pw;
        let y = |v: f64| top + ph - (v - ymin) / (ymax - ymin) * ph;
        let _ = write!(
            s,
            r##"<text x="{left}" y="{:.0}" font-size="13.5" font-weight="600" fill="#222">{title}</text><rect x="{left}" y="{top}" width="{pw}" height="{ph}" fill="none" stroke="#c9ccd1"/><text x="{:.0}" y="{:.0}" text-anchor="middle" font-size="12" fill="#444">{xlabel}</text>"##,
            top - 10.0,
            left + pw / 2.0,
            top + ph + 34.0
        );
        for k in 0..=4 {
            let v = ymin + (ymax - ymin) * f64::from(k) / 4.0;
            let _ = write!(
                s,
                r##"<line x1="{left}" x2="{:.0}" y1="{:.1}" y2="{:.1}" stroke="#eceef0"/><text x="{:.0}" y="{:.1}" text-anchor="end" font-size="11" fill="#555">{:.1}</text>"##,
                left + pw,
                y(v),
                y(v),
                left - 5.0,
                y(v) + 4.0,
                v / 1000.0
            );
            let xv = xmin + (xmax - xmin) * f64::from(k) / 4.0;
            let _ = write!(
                s,
                r##"<text x="{:.1}" y="{:.0}" text-anchor="middle" font-size="11" fill="#555">{}</text>"##,
                x(xv),
                top + ph + 16.0,
                if xmax <= 1.0 {
                    format!("{xv:.2}")
                } else if xmax > 100.0 {
                    format!("{:.1}", xv / 1000.0)
                } else {
                    format!("{xv:.0}")
                }
            );
        }
        for (pts, colour, width) in series {
            let line: Vec<String> = pts
                .iter()
                .map(|(a, b)| format!("{:.1},{:.1}", x(*a), y(b.clamp(ymin, ymax))))
                .collect();
            let _ = write!(
                s,
                r##"<polyline points="{}" fill="none" stroke="{colour}" stroke-width="{width}"/>"##,
                line.join(" ")
            );
        }
    };
    let lat: Vec<Series<'_>> = c
        .lateral
        .iter()
        .zip(colours)
        .map(|(p, col)| (p.clone(), col, 1.8))
        .collect();
    panel(
        lefts[0],
        "Lateral force (kN) against slip angle",
        "slip angle, degrees",
        0.0,
        15.0,
        0.0,
        fmax,
        &lat,
    );
    let lon: Vec<Series<'_>> = c
        .longitudinal
        .iter()
        .zip(colours)
        .map(|(p, col)| (p.clone(), col, 1.8))
        .collect();
    panel(
        lefts[1],
        "Longitudinal force (kN) against slip ratio",
        "slip ratio",
        0.0,
        0.4,
        0.0,
        fmax,
        &lon,
    );
    let comb: Vec<Series<'_>> = c
        .combined
        .iter()
        .map(|(_, p)| (p.clone(), "#d9480f", 1.2))
        .collect();
    let e = c.loads[1] * 1.4;
    panel(
        lefts[2],
        "Combined slip at the reference load (kN)",
        "longitudinal force (kN), braking to drive",
        -e,
        e,
        0.0,
        e,
        &comb,
    );
    // Legend for the loads.
    for (k, col) in colours.iter().enumerate() {
        let _ = write!(
            s,
            r##"<line x1="{:.0}" x2="{:.0}" y1="455" y2="455" stroke="{col}" stroke-width="2.5"/><text x="{:.0}" y="459" font-size="12" fill="#333">{:.0} N</text>"##,
            70.0 + 140.0 * k as f64,
            94.0 + 140.0 * k as f64,
            100.0 + 140.0 * k as f64,
            c.loads[k]
        );
    }
    let _ = write!(
        s,
        r##"<text x="860" y="459" font-size="12" fill="#333">right: slip angles 1, 2, 4, 6, 9 and 12 degrees, slip ratio -0.3 to 0.3</text></svg>"##
    );
    s
}
