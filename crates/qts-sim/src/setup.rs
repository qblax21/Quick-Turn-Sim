// SPDX-License-Identifier: Apache-2.0
//! Car setups: the adjustments a driver makes in the garage.
//!
//! A [`Setup`] is a list of changes to the car as its data file describes
//! it: tire pressures, alignment, springs, dampers, anti-roll bars, brake
//! bias, differential and gearing. Anything it does not mention stays as in
//! the file, so the empty setup is the car as delivered. Each adjustment
//! has a range a real car of the class could run ([`setup_items`]); a value
//! outside it is refused rather than clamped, so a setup file never means
//! something other than what it says.
//!
//! A setup changes parameters, not state, so it has to travel with every
//! recording: replays and ghost laps store its canonical text
//! ([`Setup::to_text`]) and a replay is rebuilt with it.

use crate::session::SessionAssets;
use qts_core::snapshot::fnv1a64;
use qts_tire::TireParams;
use qts_vehicle::params::{DifferentialKind, DrivetrainParams};
use qts_vehicle::CarParams;
use std::collections::BTreeMap;

/// Standard atmosphere, Pa. Tire pressures are set as gauge pressure; the
/// tire model uses absolute.
pub const ATMOSPHERE_PA: f64 = 101_325.0;

/// One adjustment the garage offers for a car.
#[derive(Clone, Debug, PartialEq)]
pub struct SetupItem {
    /// Key in a setup.
    pub key: String,
    /// Group heading in the garage.
    pub group: &'static str,
    /// Label in the garage.
    pub label: String,
    /// Unit shown after the value.
    pub unit: &'static str,
    /// Lowest value accepted.
    pub min: f64,
    /// Highest value accepted.
    pub max: f64,
    /// Step of the control.
    pub step: f64,
    /// Value in the car's data file.
    pub default: f64,
}

/// A named set of adjustments.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Setup {
    /// Name the driver gave it. Empty for the car as delivered.
    pub name: String,
    /// Adjusted values by key; keys not present keep the car's value.
    pub values: BTreeMap<String, f64>,
}

fn round_to(value: f64, step: f64) -> f64 {
    (value / step).round() * step
}

/// Every adjustment for a car with its range, in garage order.
///
/// Ranges are chosen as what a club-level car of the class can be set to
/// with parts a real owner would fit, not what the model can survive: see
/// `docs/SETUP.md` for where each comes from.
pub fn setup_items(
    car: &CarParams,
    tire_front: &TireParams,
    tire_rear: &TireParams,
) -> Vec<SetupItem> {
    let mut items = Vec::new();
    let mut push = |key: &str,
                    group: &'static str,
                    label: &str,
                    unit: &'static str,
                    min: f64,
                    max: f64,
                    step: f64,
                    default: f64| {
        items.push(SetupItem {
            key: key.to_string(),
            group,
            label: label.to_string(),
            unit,
            min: round_to(min, step),
            max: round_to(max, step),
            step,
            default,
        });
    };
    for (axle, name, tire) in [("front", "Front", tire_front), ("rear", "Rear", tire_rear)] {
        let gauge = (tire.pressure.cold_pressure_pa - ATMOSPHERE_PA) / 1000.0;
        push(
            &format!("{axle}.pressure_kpa"),
            "Tires",
            &format!("{name} cold pressure"),
            "kPa",
            150.0,
            300.0,
            5.0,
            gauge,
        );
    }
    for (axle, name, a) in [
        ("front", "Front", &car.suspension.front),
        ("rear", "Rear", &car.suspension.rear),
    ] {
        push(
            &format!("{axle}.camber_deg"),
            "Alignment",
            &format!("{name} camber"),
            "deg",
            -4.0,
            0.5,
            0.1,
            a.static_camber_rad.to_degrees(),
        );
        push(
            &format!("{axle}.toe_deg"),
            "Alignment",
            &format!("{name} toe per wheel (+ in)"),
            "deg",
            -0.5,
            0.5,
            0.02,
            a.static_toe_rad.to_degrees(),
        );
    }
    for (axle, name, a) in [
        ("front", "Front", &car.suspension.front),
        ("rear", "Rear", &car.suspension.rear),
    ] {
        let k = a.spring_rate_n_per_m / 1000.0;
        push(
            &format!("{axle}.spring_n_per_mm"),
            "Springs",
            &format!("{name} spring"),
            "N/mm",
            0.6 * k,
            2.5 * k,
            1.0,
            k,
        );
    }
    for (axle, name, a) in [
        ("front", "Front", &car.suspension.front),
        ("rear", "Rear", &car.suspension.rear),
    ] {
        push(
            &format!("{axle}.bump_n_s_per_m"),
            "Dampers",
            &format!("{name} bump"),
            "N.s/m",
            0.5 * a.damper_bump_n_s_per_m,
            2.0 * a.damper_bump_n_s_per_m,
            50.0,
            a.damper_bump_n_s_per_m,
        );
        push(
            &format!("{axle}.rebound_n_s_per_m"),
            "Dampers",
            &format!("{name} rebound"),
            "N.s/m",
            0.5 * a.damper_rebound_n_s_per_m,
            2.0 * a.damper_rebound_n_s_per_m,
            50.0,
            a.damper_rebound_n_s_per_m,
        );
    }
    for (axle, name, a) in [
        ("front", "Front", &car.suspension.front),
        ("rear", "Rear", &car.suspension.rear),
    ] {
        let k = a.anti_roll_bar_n_per_m / 1000.0;
        push(
            &format!("{axle}.arb_n_per_mm"),
            "Anti-roll bars",
            &format!("{name} bar (0 = disconnected)"),
            "N/mm",
            0.0,
            (2.5 * k).max(20.0),
            0.5,
            k,
        );
    }
    let b = &car.brakes;
    let total = b.max_torque_front_nm + b.max_torque_rear_nm;
    let bias = 100.0 * b.max_torque_front_nm / total;
    push(
        "brake_bias_pct",
        "Brakes",
        "Brake bias to the front",
        "%",
        50.0,
        85.0_f64.max(bias + 5.0),
        0.5,
        bias,
    );
    if let DrivetrainParams::EngineManual(m) = &car.drivetrain {
        let d = &m.differential;
        if d.kind == DifferentialKind::LimitedSlip {
            push(
                "diff.preload_nm",
                "Differential",
                "Preload",
                "N.m",
                0.0,
                200.0,
                5.0,
                d.preload_nm,
            );
            push(
                "diff.lock_drive_pct",
                "Differential",
                "Locking on power",
                "%",
                0.0,
                80.0,
                1.0,
                100.0 * d.lock_fraction_drive,
            );
            push(
                "diff.lock_coast_pct",
                "Differential",
                "Locking off power",
                "%",
                0.0,
                80.0,
                1.0,
                100.0 * d.lock_fraction_coast,
            );
        }
        let cl = &m.clutch;
        push(
            "clutch.bite_point_pct",
            "Clutch",
            "Bite point (pedal travel from the top)",
            "%",
            30.0,
            65.0,
            1.0,
            100.0 * cl.bite_point,
        );
        push(
            "clutch.bite_width_pct",
            "Clutch",
            "Bite zone width",
            "%",
            10.0,
            40.0,
            1.0,
            100.0 * cl.bite_width,
        );
        let g = &m.gearbox;
        push(
            "final_drive",
            "Gearing",
            "Final drive",
            ":1",
            0.85 * g.final_drive,
            1.15 * g.final_drive,
            0.01,
            g.final_drive,
        );
        for (i, r) in g.ratios.iter().enumerate() {
            push(
                &format!("gear_{}", i + 1),
                "Gearing",
                &format!("Gear {}", i + 1),
                ":1",
                0.9 * r,
                1.1 * r,
                0.01,
                *r,
            );
        }
    }
    items
}

impl Setup {
    /// Canonical text: one `key = value` line per adjustment, sorted, with
    /// the name first. Two setups with the same text drive the same.
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        if !self.name.is_empty() {
            text.push_str(&format!("name = {}\n", self.name.replace('\n', " ")));
        }
        for (k, v) in &self.values {
            text.push_str(&format!("{k} = {v}\n"));
        }
        text
    }

    /// Reads [`Setup::to_text`] back. Lines starting with `#` are comments.
    pub fn from_text(text: &str) -> Result<Self, String> {
        let mut setup = Setup::default();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (k, v) = line
                .split_once('=')
                .ok_or_else(|| format!("line {}: expected `key = value`", n + 1))?;
            let (k, v) = (k.trim(), v.trim());
            if k == "name" {
                setup.name = v.to_string();
                continue;
            }
            let value: f64 = v
                .parse()
                .ok()
                .filter(|x: &f64| x.is_finite())
                .ok_or_else(|| format!("line {}: `{v}` is not a number", n + 1))?;
            setup.values.insert(k.to_string(), value);
        }
        Ok(setup)
    }

    /// Hash of the adjustments (not the name).
    pub fn hash(&self) -> u64 {
        let unnamed = Setup {
            name: String::new(),
            values: self.values.clone(),
        };
        fnv1a64(unnamed.to_text().as_bytes())
    }

    /// Name to show: the given name, or "default" for the car as
    /// delivered, or "unnamed".
    pub fn display_name(&self) -> String {
        if !self.name.is_empty() {
            self.name.clone()
        } else if self.values.is_empty() {
            "default".into()
        } else {
            "unnamed".into()
        }
    }

    /// Applies the setup to a car and its tires. Every key must be one the
    /// car offers and every value inside its range.
    pub fn apply(
        &self,
        car: &mut CarParams,
        tire_front: &mut TireParams,
        tire_rear: &mut TireParams,
    ) -> Result<(), String> {
        let items = setup_items(car, tire_front, tire_rear);
        for (key, &value) in &self.values {
            let item = items
                .iter()
                .find(|i| &i.key == key)
                .ok_or_else(|| format!("setup: `{key}` cannot be adjusted on {}", car.name))?;
            // Half a step of slack for values written with rounding.
            let slack = 0.5 * item.step;
            if value < item.min - slack || value > item.max + slack {
                return Err(format!(
                    "setup: {} = {value} {} is outside the range {} to {}",
                    item.label, item.unit, item.min, item.max
                ));
            }
        }
        let get = |key: &str| self.values.get(key).copied();
        if let Some(v) = get("front.pressure_kpa") {
            tire_front.pressure.cold_pressure_pa = v * 1000.0 + ATMOSPHERE_PA;
        }
        if let Some(v) = get("rear.pressure_kpa") {
            tire_rear.pressure.cold_pressure_pa = v * 1000.0 + ATMOSPHERE_PA;
        }
        for (axle, a) in [
            ("front", &mut car.suspension.front),
            ("rear", &mut car.suspension.rear),
        ] {
            if let Some(v) = get(&format!("{axle}.camber_deg")) {
                a.static_camber_rad = v.to_radians();
            }
            if let Some(v) = get(&format!("{axle}.toe_deg")) {
                a.static_toe_rad = v.to_radians();
            }
            if let Some(v) = get(&format!("{axle}.spring_n_per_mm")) {
                a.spring_rate_n_per_m = v * 1000.0;
            }
            if let Some(v) = get(&format!("{axle}.bump_n_s_per_m")) {
                a.damper_bump_n_s_per_m = v;
            }
            if let Some(v) = get(&format!("{axle}.rebound_n_s_per_m")) {
                a.damper_rebound_n_s_per_m = v;
            }
            if let Some(v) = get(&format!("{axle}.arb_n_per_mm")) {
                a.anti_roll_bar_n_per_m = v * 1000.0;
            }
        }
        if let Some(v) = get("brake_bias_pct") {
            let b = &mut car.brakes;
            let total = b.max_torque_front_nm + b.max_torque_rear_nm;
            b.max_torque_front_nm = total * v / 100.0;
            b.max_torque_rear_nm = total - b.max_torque_front_nm;
        }
        if let DrivetrainParams::EngineManual(m) = &mut car.drivetrain {
            if let Some(v) = get("diff.preload_nm") {
                m.differential.preload_nm = v;
            }
            if let Some(v) = get("diff.lock_drive_pct") {
                m.differential.lock_fraction_drive = v / 100.0;
            }
            if let Some(v) = get("diff.lock_coast_pct") {
                m.differential.lock_fraction_coast = v / 100.0;
            }
            if let Some(v) = get("clutch.bite_point_pct") {
                m.clutch.bite_point = v / 100.0;
            }
            if let Some(v) = get("clutch.bite_width_pct") {
                m.clutch.bite_width = v / 100.0;
            }
            if let Some(v) = get("final_drive") {
                m.gearbox.final_drive = v;
            }
            for (i, r) in m.gearbox.ratios.iter_mut().enumerate() {
                if let Some(v) = get(&format!("gear_{}", i + 1)) {
                    *r = v;
                }
            }
            if m.gearbox.ratios.windows(2).any(|w| w[1] >= w[0]) {
                return Err("setup: each gear must be taller than the one below it".into());
            }
        }
        car.validate().map_err(|e| format!("setup: {e}"))
    }
}

impl SessionAssets {
    /// Applies a setup to the car and tires these assets hold, and
    /// remembers it so recordings carry it.
    pub fn with_setup(mut self, setup: &Setup) -> Result<Self, String> {
        setup.apply(&mut self.car, &mut self.tire_front, &mut self.tire_rear)?;
        self.setup = setup.clone();
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trips_and_ignores_comments() {
        let mut s = Setup {
            name: "Ridgeline quali".into(),
            ..Setup::default()
        };
        s.values.insert("front.camber_deg".into(), -2.5);
        s.values.insert("brake_bias_pct".into(), 64.5);
        let text = s.to_text();
        assert_eq!(Setup::from_text(&format!("# saved\n{text}")).unwrap(), s);
        assert!(Setup::from_text("front.camber_deg = soft").is_err());
        assert!(Setup::from_text("nonsense").is_err());
        let mut renamed = s.clone();
        renamed.name = "other".into();
        assert_eq!(renamed.hash(), s.hash());
        assert_eq!(Setup::default().display_name(), "default");
    }
}
