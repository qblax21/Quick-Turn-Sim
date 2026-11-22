// SPDX-License-Identifier: Apache-2.0
//! Cold tires on the first lap from the pits (D-108). Drives a lap with the
//! tires starting at a given temperature and reads back, corner by corner,
//! the tire temperatures and the grip factor the thermal model gives them
//! (`ThermalState::grip_factor`). It also tabulates the model's grip and
//! stiffness factors against temperature, so they can be set beside
//! published figures. Nothing here changes the simulation.

use crate::coach::{find_corners, Corner};
use crate::data::DataRoot;
use crate::drivers::{Driver, PathFollower, PathFollowerConfig};
use crate::lap_telemetry::{Lap, LapRecorder};
use crate::session::{Session, SessionConfig};
use qts_core::step::Simulation;
use qts_ffb::FfbSample;
use qts_tire::{ThermalState, TireParams};
use qts_vehicle::Assists;
use std::fmt::Write as _;

/// Wheel prefixes in telemetry, front axle first.
const WHEELS: [&str; 4] = ["fl", "fr", "rl", "rr"];

/// One driven lap with its thermal trace.
#[derive(Clone, Debug)]
pub struct ThermalLap {
    /// Tire temperature at the start, °C.
    pub start_c: f64,
    /// Lap time, s.
    pub lap_s: f64,
    /// The lap's telemetry.
    pub lap: Lap,
    /// Per row: mean of the four tires' grip factors.
    pub grip: Vec<f64>,
    /// Per row: mean tread surface, tread core and carcass temperature of
    /// the four tires, °C.
    pub temps: Vec<[f64; 3]>,
}

impl ThermalLap {
    /// Grip factor at a distance along this lap.
    pub fn grip_at(&self, d: f64) -> f64 {
        self.lap.at(&self.grip, d)
    }

    /// Temperatures (surface, core, carcass) at a distance, °C.
    pub fn temps_at(&self, d: f64) -> [f64; 3] {
        let col = |k: usize| self.temps.iter().map(|t| t[k]).collect::<Vec<f64>>();
        [0, 1, 2].map(|k| self.lap.at(&col(k), d))
    }
}

/// Drives laps of `track` in `car` from the pits with the tires at
/// `start_c`, by the path follower planning corners for `lateral_accel`
/// (m/s²), and returns the first `laps` counted laps.
pub fn drive(
    root: &DataRoot,
    car: &str,
    track: &str,
    start_c: f64,
    lateral_accel: f64,
    laps: u32,
) -> Result<Vec<ThermalLap>, String> {
    let dir = std::env::temp_dir().join(format!(
        "qts_cold_start_{car}_{track}_{start_c}_{lateral_accel}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let config = SessionConfig {
        assists: Assists {
            auto_clutch: true,
            auto_shift: true,
            traction_control: true,
            engine_damage: false,
        },
        tire_start_temp_c: Some(start_c),
        ..SessionConfig::default()
    };
    let mut s = Session::from_data(root, car, track, config)?;
    let params = [
        s.car().tire_params(0).clone(),
        s.car().tire_params(1).clone(),
    ];
    let path = s
        .track()
        .paths()
        .first()
        .ok_or("the track has no path to drive")?
        .clone();
    let mut driver = PathFollower::new(
        &path,
        PathFollowerConfig {
            closed: true,
            lateral_accel,
            ..PathFollowerConfig::default()
        },
    );
    let mut recorder = LapRecorder::new(&dir, &s)?;
    for _ in 0..(laps as usize * 300_000) {
        let frame = driver.frame(&s);
        s.step(&frame);
        recorder.observe(&s, &frame, &FfbSample::default());
        if s.timing().runs >= laps {
            break;
        }
    }
    let mut out = Vec::new();
    for file in recorder.wait() {
        let lap = Lap::read(&file?)?;
        out.push(thermal_lap(lap, &params, start_c)?);
    }
    let _ = std::fs::remove_dir_all(&dir);
    if out.len() < laps as usize {
        return Err(format!("only {} of {laps} laps were completed", out.len()));
    }
    Ok(out)
}

fn thermal_lap(lap: Lap, params: &[TireParams; 2], start_c: f64) -> Result<ThermalLap, String> {
    let col = |name: String| lap.file.column(&name).ok_or(format!("no {name} column"));
    let mut cols = Vec::new();
    for w in WHEELS {
        cols.push([
            col(format!("{w}_temp_surface_c"))?,
            col(format!("{w}_temp_core_c"))?,
            col(format!("{w}_temp_carcass_c"))?,
        ]);
    }
    let n = lap.time.len();
    let mut grip = Vec::with_capacity(n);
    let mut temps = Vec::with_capacity(n);
    for i in 0..n {
        let (mut g, mut t) = (0.0, [0.0; 3]);
        for (k, c) in cols.iter().enumerate() {
            let state = ThermalState {
                surface_c: c[0][i],
                core_c: c[1][i],
                carcass_c: c[2][i],
                gas_c: c[2][i],
            };
            g += 0.25 * state.grip_factor(&params[k / 2].thermal);
            for j in 0..3 {
                t[j] += 0.25 * c[j][i];
            }
        }
        grip.push(g);
        temps.push(t);
    }
    Ok(ThermalLap {
        start_c,
        lap_s: *lap.time.last().unwrap_or(&0.0),
        lap,
        grip,
        temps,
    })
}

/// A cold first lap against a warm one, corner by corner.
#[derive(Clone, Debug)]
pub struct WarmUp {
    /// Corners of the lap (from the warm lap).
    pub corners: Vec<Corner>,
    /// Per corner, at its exit: cold lap's grip factor and the warm lap's.
    pub grip: Vec<(f64, f64)>,
    /// Per corner, at its exit: the cold lap's mean surface, core and
    /// carcass temperature, °C.
    pub cold_temps: Vec<[f64; 3]>,
    /// Corners driven before the cold tires' grip stays within
    /// `tolerance` of the warm tires' at every later corner exit. `None`
    /// if that never happens on the lap.
    pub warm_after: Option<usize>,
}

/// Compares the cold lap with the warm one at each corner's exit.
pub fn warm_up(cold: &ThermalLap, warm: &ThermalLap, tolerance: f64) -> Result<WarmUp, String> {
    let corners = find_corners(&warm.lap)?;
    let scale = cold.lap.length() / warm.lap.length();
    let mut grip = Vec::new();
    let mut cold_temps = Vec::new();
    for c in &corners {
        let d = c.curve.1;
        grip.push((cold.grip_at(d * scale), warm.grip_at(d)));
        cold_temps.push(cold.temps_at(d * scale));
    }
    let close: Vec<bool> = grip
        .iter()
        .map(|(c, w)| (c - w).abs() <= tolerance)
        .collect();
    let warm_after = (0..=close.len())
        .find(|&k| close[k..].iter().all(|x| *x))
        .filter(|&k| k < close.len());
    Ok(WarmUp {
        corners,
        grip,
        cold_temps,
        warm_after,
    })
}

/// The model's grip factor and stiffness factor against a uniform tire
/// temperature, °C, from `from` to `to` in `step`.
pub fn factors(p: &TireParams, from: f64, to: f64, step: f64) -> Vec<(f64, f64, f64)> {
    let n = ((to - from) / step).round() as usize;
    (0..=n)
        .map(|i| {
            let t = from + step * i as f64;
            let s = ThermalState::uniform(t);
            (t, s.grip_factor(&p.thermal), s.stiffness_factor(&p.thermal))
        })
        .collect()
}

/// Relative change of a factor between two uniform temperatures.
pub fn change(p: &TireParams, from_c: f64, to_c: f64) -> (f64, f64) {
    let (a, b) = (ThermalState::uniform(from_c), ThermalState::uniform(to_c));
    (
        b.grip_factor(&p.thermal) / a.grip_factor(&p.thermal) - 1.0,
        b.stiffness_factor(&p.thermal) / a.stiffness_factor(&p.thermal) - 1.0,
    )
}

/// Published cold-to-hot changes for a high-performance summer tire on a
/// Flat-Trac machine, from about 30 to about 70 °C surface: peak grip
/// about 10 % lower, cornering stiffness 20 to 25 % lower. Singh and
/// Sivaramakrishnan (Goodyear), "Extended Pacejka Tire Model for Enhanced
/// Vehicle Stability Control", arXiv:2305.18422 (2023), Table 3 and
/// Figure 26.
pub const PUBLISHED_HOT_C: (f64, f64) = (30.0, 70.0);
/// Peak grip change, cold to hot (published).
pub const PUBLISHED_GRIP_CHANGE: f64 = -0.10;
/// Cornering stiffness change, cold to hot (published range).
pub const PUBLISHED_STIFFNESS_CHANGE: (f64, f64) = (-0.25, -0.20);

/// Two panels: the model's factors against temperature with the
/// published cold-to-hot changes marked, and a cold first lap's mean
/// temperatures and grip factor against distance beside a warm one's.
pub fn svg(
    tire: &TireParams,
    cold: &ThermalLap,
    warm: &ThermalLap,
    w: &WarmUp,
    title: &str,
) -> String {
    let (wd, ht) = (1200.0, 500.0);
    let (pw, ph, top) = (480.0, 320.0, 100.0);
    let mut s = String::new();
    let _ = write!(
        s,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{wd}" height="{ht}" viewBox="0 0 {wd} {ht}" font-family="Segoe UI, Arial, sans-serif"><rect width="{wd}" height="{ht}" fill="#ffffff"/><text x="70" y="30" font-size="17" font-weight="700" fill="#111">{title}</text><text x="70" y="52" font-size="12.5" fill="#555">Left: the thermal model's factors at a uniform tire temperature; the dashed boxes are a published cold-to-hot change (Singh and Sivaramakrishnan, arXiv:2305.18422).</text><text x="70" y="70" font-size="12.5" fill="#555">Right: the first lap from the pits with the tires at {:.0} °C (solid) and at {:.0} °C (dotted); grip is the mean of the four tires' factors.</text>"##,
        cold.start_c, warm.start_c
    );
    // Panel helper.
    let mut frame = |left: f64,
                     title: &str,
                     xlabel: &str,
                     xr: (f64, f64),
                     yr: (f64, f64),
                     yfmt: &dyn Fn(f64) -> String| {
        let _ = write!(
            s,
            r##"<text x="{left}" y="{:.0}" font-size="13.5" font-weight="600" fill="#222">{title}</text><rect x="{left}" y="{top}" width="{pw}" height="{ph}" fill="none" stroke="#c9ccd1"/><text x="{:.0}" y="{:.0}" text-anchor="middle" font-size="12" fill="#444">{xlabel}</text>"##,
            top - 10.0,
            left + pw / 2.0,
            top + ph + 34.0
        );
        for k in 0..=4 {
            let fy = f64::from(k) / 4.0;
            let yv = yr.0 + (yr.1 - yr.0) * fy;
            let yy = top + ph - fy * ph;
            let xv = xr.0 + (xr.1 - xr.0) * fy;
            let _ = write!(
                s,
                r##"<line x1="{left}" x2="{:.0}" y1="{yy:.1}" y2="{yy:.1}" stroke="#eceef0"/><text x="{:.0}" y="{:.1}" text-anchor="end" font-size="11" fill="#555">{}</text><text x="{:.1}" y="{:.0}" text-anchor="middle" font-size="11" fill="#555">{xv:.0}</text>"##,
                left + pw,
                left - 5.0,
                yy + 4.0,
                yfmt(yv),
                left + fy * pw,
                top + ph + 16.0
            );
        }
    };
    let (l1, l2) = (70.0, 650.0);
    frame(
        l1,
        "Grip and stiffness factor against temperature",
        "tire temperature, °C",
        (0.0, 120.0),
        (0.7, 1.1),
        &|v| format!("{v:.2}"),
    );
    frame(
        l2,
        "First lap from the pits: temperatures (°C, left axis) and grip",
        "distance, m",
        (0.0, warm.lap.length()),
        (0.0, 80.0),
        &|v| format!("{v:.0}"),
    );
    let poly = |pts: &[(f64, f64)], colour: &str, dash: &str, width: f64| -> String {
        let p: Vec<String> = pts.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
        format!(
            r##"<polyline points="{}" fill="none" stroke="{colour}" stroke-width="{width}" stroke-dasharray="{dash}"/>"##,
            p.join(" ")
        )
    };
    // Left panel.
    let x1 = |t: f64| l1 + t / 120.0 * pw;
    let y1 = |f: f64| top + ph - (f.clamp(0.7, 1.1) - 0.7) / 0.4 * ph;
    let f = factors(tire, 0.0, 120.0, 2.0);
    let grip: Vec<(f64, f64)> = f.iter().map(|(t, g, _)| (x1(*t), y1(*g))).collect();
    let stiff: Vec<(f64, f64)> = f.iter().map(|(t, _, k)| (x1(*t), y1(*k))).collect();
    s.push_str(&poly(&grip, "#d9480f", "", 2.0));
    s.push_str(&poly(&stiff, "#1f6feb", "", 2.0));
    // Published changes from 30 to 70 °C, drawn from the model's value at
    // 30 °C so the slopes can be compared.
    let (a, b) = PUBLISHED_HOT_C;
    let base = ThermalState::uniform(a);
    let g0 = base.grip_factor(&tire.thermal);
    let k0 = base.stiffness_factor(&tire.thermal);
    let _ = write!(
        s,
        r##"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="#d9480f" stroke-width="1.5" stroke-dasharray="5 4"/><rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="#1f6feb" fill-opacity="0.12" stroke="#1f6feb" stroke-dasharray="5 4"/>"##,
        x1(a),
        y1(g0),
        x1(b),
        y1(g0 * (1.0 + PUBLISHED_GRIP_CHANGE)),
        x1(b) - 4.0,
        y1(k0 * (1.0 + PUBLISHED_STIFFNESS_CHANGE.1)),
        8.0,
        y1(k0 * (1.0 + PUBLISHED_STIFFNESS_CHANGE.0))
            - y1(k0 * (1.0 + PUBLISHED_STIFFNESS_CHANGE.1))
    );
    let _ = write!(
        s,
        r##"<text x="{:.0}" y="{:.0}" font-size="12" fill="#d9480f">grip factor (surface and core)</text><text x="{:.0}" y="{:.0}" font-size="12" fill="#1f6feb">stiffness factor (carcass)</text><text x="{:.0}" y="{:.0}" font-size="11" fill="#555">dashed: published 30 to 70 °C change</text>"##,
        l1 + 10.0,
        top + ph - 44.0,
        l1 + 10.0,
        top + ph - 28.0,
        l1 + 10.0,
        top + ph - 12.0
    );
    // Right panel.
    let len = warm.lap.length();
    let x2 = |d: f64| l2 + d / len * pw;
    let y2 = |t: f64| top + ph - t.clamp(0.0, 80.0) / 80.0 * ph;
    let yg = |g: f64| top + ph - (g.clamp(0.9, 1.0) - 0.9) / 0.1 * ph;
    for c in &w.corners {
        let _ = write!(
            s,
            r##"<rect x="{:.1}" y="{top}" width="{:.1}" height="{ph}" fill="#f3f4f6"/><text x="{:.1}" y="{:.0}" font-size="10" fill="#777" text-anchor="middle">T{}</text>"##,
            x2(c.curve.0),
            (x2(c.curve.1) - x2(c.curve.0)).max(1.0),
            x2(0.5 * (c.curve.0 + c.curve.1)),
            top + 12.0,
            c.number
        );
    }
    for (lap, dash) in [(cold, ""), (warm, "2 3")] {
        let scale = lap.lap.length() / len;
        let step = (lap.lap.distance.len() / 400).max(1);
        let rows: Vec<usize> = (0..lap.lap.distance.len()).step_by(step).collect();
        for (k, colour) in [(0, "#d9480f"), (1, "#a35d00"), (2, "#1f6feb")] {
            let pts: Vec<(f64, f64)> = rows
                .iter()
                .map(|&i| (x2(lap.lap.distance[i] / scale), y2(lap.temps[i][k])))
                .collect();
            s.push_str(&poly(&pts, colour, dash, 1.6));
        }
        let pts: Vec<(f64, f64)> = rows
            .iter()
            .map(|&i| (x2(lap.lap.distance[i] / scale), yg(lap.grip[i])))
            .collect();
        s.push_str(&poly(&pts, "#2b8a3e", dash, 2.0));
    }
    let _ = write!(
        s,
        r##"<text x="{:.0}" y="{:.0}" font-size="12" fill="#d9480f">surface</text><text x="{:.0}" y="{:.0}" font-size="12" fill="#a35d00">core</text><text x="{:.0}" y="{:.0}" font-size="12" fill="#1f6feb">carcass</text><text x="{:.0}" y="{:.0}" font-size="12" fill="#2b8a3e">grip factor (0.90 at the bottom to 1.00 at the top)</text>"##,
        l2 + 10.0,
        top + 0.3 * ph,
        l2 + 70.0,
        top + 0.3 * ph,
        l2 + 115.0,
        top + 0.3 * ph,
        l2 + 10.0,
        top + 0.3 * ph + 18.0
    );
    s.push_str("</svg>\n");
    s
}
