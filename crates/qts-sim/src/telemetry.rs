// SPDX-License-Identifier: Apache-2.0
//! Telemetry channels: which columns a session logs and how each row is
//! filled.
//!
//! The channel list here is the single source of truth. A test checks that
//! `docs/TELEMETRY_FORMAT.md` lists every channel.

use crate::session::Session;
use qts_core::constants::{CORE_VERSION, PRODUCT_NAME};
use qts_core::input::InputFrame;
use qts_core::step::Simulation;
use qts_ffb::FfbSample;
use qts_telemetry::Channel;

macro_rules! ch {
    ($name:expr, $unit:expr, $desc:expr) => {
        Channel {
            name: $name,
            unit: $unit,
            description: $desc,
        }
    };
}

macro_rules! wheel_channels {
    ($p:literal, $w:literal) => {
        [
            ch!(
                concat!($p, "_slip_ratio"),
                "-",
                concat!("Transient slip ratio, ", $w)
            ),
            ch!(
                concat!($p, "_slip_angle_rad"),
                "rad",
                concat!("Transient slip angle, ", $w)
            ),
            ch!(
                concat!($p, "_load_n"),
                "N",
                concat!("Vertical tire load, ", $w)
            ),
            ch!(
                concat!($p, "_fx_n"),
                "N",
                concat!("Longitudinal tire force in the wheel frame, ", $w)
            ),
            ch!(
                concat!($p, "_fy_n"),
                "N",
                concat!("Lateral tire force in the wheel frame, positive left, ", $w)
            ),
            ch!(
                concat!($p, "_mz_nm"),
                "N.m",
                concat!("Tire aligning moment, ", $w)
            ),
            ch!(
                concat!($p, "_omega_rad_s"),
                "rad/s",
                concat!("Wheel spin rate, ", $w)
            ),
            ch!(
                concat!($p, "_susp_travel_m"),
                "m",
                concat!("Suspension travel, positive in bump, ", $w)
            ),
            ch!(
                concat!($p, "_camber_rad"),
                "rad",
                concat!("Camber relative to the road, toward the car's left, ", $w)
            ),
            ch!(
                concat!($p, "_temp_surface_c"),
                "degC",
                concat!("Tread surface temperature, ", $w)
            ),
            ch!(
                concat!($p, "_temp_core_c"),
                "degC",
                concat!("Tread core temperature, ", $w)
            ),
            ch!(
                concat!($p, "_temp_carcass_c"),
                "degC",
                concat!("Carcass temperature, ", $w)
            ),
            ch!(
                concat!($p, "_pressure_pa"),
                "Pa",
                concat!("Absolute inflation pressure, ", $w)
            ),
            ch!(
                concat!($p, "_brake_torque_nm"),
                "N.m",
                concat!("Friction-brake torque applied, ", $w)
            ),
            ch!(
                concat!($p, "_drive_torque_nm"),
                "N.m",
                concat!("Driveline torque received, ", $w)
            ),
            ch!(
                concat!($p, "_surface"),
                "-",
                concat!("Index of the surface under the wheel, ", $w)
            ),
        ]
    };
}

const CHASSIS: [Channel; 34] = [
    ch!("time_s", "s", "Simulated time since the session started"),
    ch!("tick", "-", "Physics tick number"),
    ch!("pos_x_m", "m", "World x of the sprung centre of gravity"),
    ch!("pos_y_m", "m", "World y of the sprung centre of gravity"),
    ch!(
        "pos_z_m",
        "m",
        "World z (height) of the sprung centre of gravity"
    ),
    ch!(
        "yaw_rad",
        "rad",
        "Heading, counter-clockwise from the world x axis"
    ),
    ch!("pitch_rad", "rad", "Pitch, nose up positive"),
    ch!("roll_rad", "rad", "Roll, left side up positive"),
    ch!("speed_m_s", "m/s", "Speed over the ground"),
    ch!("vel_long_m_s", "m/s", "Velocity along the car's x axis"),
    ch!(
        "vel_lat_m_s",
        "m/s",
        "Velocity along the car's y axis, positive left"
    ),
    ch!(
        "accel_long_m_s2",
        "m/s2",
        "Acceleration along the car's x axis"
    ),
    ch!(
        "accel_lat_m_s2",
        "m/s2",
        "Acceleration along the car's y axis, positive left"
    ),
    ch!(
        "accel_vert_m_s2",
        "m/s2",
        "Acceleration along the car's z axis"
    ),
    ch!(
        "yaw_rate_rad_s",
        "rad/s",
        "Yaw rate, positive counter-clockwise"
    ),
    ch!(
        "steer_input",
        "-",
        "Steering input, -1 full right to 1 full left"
    ),
    ch!("throttle", "-", "Accelerator input, 0 to 1"),
    ch!("brake", "-", "Brake pedal input, 0 to 1"),
    ch!("clutch", "-", "Clutch pedal input, 0 engaged to 1 pressed"),
    ch!("handbrake", "-", "Handbrake input, 0 to 1"),
    ch!(
        "steering_wheel_rad",
        "rad",
        "Steering-wheel angle, positive left"
    ),
    ch!(
        "gear",
        "-",
        "Selected gear: -1 reverse, 0 neutral, 1.. forward"
    ),
    ch!(
        "engine_rpm",
        "rpm",
        "Engine speed, or front motor speed for an electric car"
    ),
    ch!(
        "engine_torque_nm",
        "N.m",
        "Engine torque, or summed motor torque at the rotors"
    ),
    ch!("clutch_torque_nm", "N.m", "Torque through the clutch"),
    ch!(
        "drive_power_w",
        "W",
        "Mechanical power delivered to the wheels"
    ),
    ch!(
        "regen_power_w",
        "W",
        "Mechanical power recovered by regeneration"
    ),
    ch!(
        "battery_power_w",
        "W",
        "Electrical power drawn from the battery, negative while charging"
    ),
    ch!("battery_soc", "-", "Battery state of charge, 0 to 1"),
    ch!("motor_front_torque_nm", "N.m", "Front motor rotor torque"),
    ch!("motor_rear_torque_nm", "N.m", "Rear motor rotor torque"),
    ch!("aero_drag_n", "N", "Aerodynamic drag force"),
    ch!(
        "run_time_s",
        "s",
        "Time in the current timed run, zero when none is active"
    ),
    ch!("cones_hit", "-", "Cones knocked down in the current run"),
];

const FFB: [Channel; 10] = [
    ch!(
        "steering_torque_nm",
        "N.m",
        "Raw steering torque at the column from the physics, positive pushes the wheel left"
    ),
    ch!(
        "ffb_raw_nm",
        "N.m",
        "Force-feedback pipeline input (pre-filter), equal to steering_torque_nm"
    ),
    ch!("ffb_scaled_nm", "N.m", "After gain, before filtering"),
    ch!(
        "ffb_filtered_nm",
        "N.m",
        "After the low-pass filter and slew limiter"
    ),
    ch!(
        "ffb_effects_nm",
        "N.m",
        "Damping, friction and inertia effects"
    ),
    ch!("ffb_detail_nm", "N.m", "Road-detail and kerb channels"),
    ch!(
        "ffb_requested",
        "-",
        "Requested output as a fraction of device peak torque, before the soft clip"
    ),
    ch!(
        "ffb_output",
        "-",
        "Output after the soft clip (post-filter), -1 to 1"
    ),
    ch!("ffb_clipped", "-", "Amount removed by the soft clip"),
    ch!(
        "ffb_wheel_velocity_rad_s",
        "rad/s",
        "Filtered velocity of the physical wheel"
    ),
];

const WHEELS: [[Channel; 16]; 4] = [
    wheel_channels!("fl", "front left"),
    wheel_channels!("fr", "front right"),
    wheel_channels!("rl", "rear left"),
    wheel_channels!("rr", "rear right"),
];

/// Every telemetry channel, in column order.
pub fn channels() -> Vec<Channel> {
    let mut all = Vec::with_capacity(CHASSIS.len() + 64 + FFB.len());
    all.extend_from_slice(&CHASSIS);
    for wheel in &WHEELS {
        all.extend_from_slice(wheel);
    }
    all.extend_from_slice(&FFB);
    all
}

/// Product name and version for the `generator` line of a telemetry file.
pub fn generator() -> String {
    format!("{PRODUCT_NAME} {CORE_VERSION}")
}

/// Metadata lines describing a session.
pub fn metadata(session: &Session, sample_rate_hz: f64) -> Vec<(String, String)> {
    let car = session.car().params();
    let track = session.track();
    let surfaces: Vec<String> = track
        .surfaces()
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{i}:{}", s.id))
        .collect();
    vec![
        ("car".into(), car.id.clone()),
        ("car_name".into(), car.name.clone()),
        ("track".into(), track.def().id.clone()),
        ("tick_rate_hz".into(), session.tick_rate_hz().to_string()),
        ("sample_rate_hz".into(), format!("{sample_rate_hz}")),
        (
            "content_hash".into(),
            format!("{:016x}", session.content_hash()),
        ),
        ("surfaces".into(), surfaces.join(" ")),
        (
            "note".into(),
            "all vehicle, tire and surface parameters are placeholders unless marked measured"
                .into(),
        ),
    ]
}

/// Fills `row` with the values of every channel for the session's last
/// tick. `input` is the frame that tick consumed and `ffb` the pipeline's
/// sample for it.
pub fn fill_row(session: &Session, input: &InputFrame, ffb: &FfbSample, row: &mut Vec<f64>) {
    row.clear();
    let car = session.car();
    let s = &car.state;
    let o = car.output();
    row.extend_from_slice(&[
        session.sim_time(),
        session.tick() as f64,
        s.pos.x,
        s.pos.y,
        s.pos.z,
        s.rot.yaw(),
        s.rot.pitch(),
        s.rot.roll(),
        o.speed,
        o.vel_body[0],
        o.vel_body[1],
        o.accel_body[0],
        o.accel_body[1],
        o.accel_body[2],
        o.yaw_rate,
        input.steer_f64(),
        input.throttle_f64(),
        input.brake_f64(),
        input.clutch_f64(),
        input.handbrake_f64(),
        o.steering_wheel_angle,
        f64::from(o.drive.gear),
        o.drive.engine_rpm,
        o.drive.engine_torque_nm,
        o.drive.clutch_torque_nm,
        o.drive.drive_power_w,
        o.drive.regen_power_w,
        o.drive.battery_power_w,
        o.drive.battery_soc,
        o.drive.motor_torque_nm[0],
        o.drive.motor_torque_nm[1],
        o.aero_drag_n,
        session.run_time(),
        f64::from(session.timing().cones_hit),
    ]);
    for w in &o.wheels {
        row.extend_from_slice(&[
            w.slip_ratio,
            w.slip_angle,
            w.load,
            w.fx,
            w.fy,
            w.mz,
            w.omega,
            w.susp_travel,
            w.camber,
            w.temp_surface_c,
            w.temp_core_c,
            w.temp_carcass_c,
            w.pressure_pa,
            w.brake_torque,
            w.drive_torque,
            f64::from(w.surface_index),
        ]);
    }
    row.extend_from_slice(&[
        o.steering_torque_nm,
        ffb.raw_nm,
        ffb.scaled_nm,
        ffb.filtered_nm,
        ffb.effects_nm,
        ffb.detail_nm,
        ffb.requested,
        ffb.output,
        ffb.clipped,
        ffb.wheel_velocity_rad_s,
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_names_are_unique_and_well_formed() {
        let all = channels();
        assert_eq!(all.len(), 34 + 64 + 10);
        let mut names: Vec<&str> = all.iter().map(|c| c.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len(), "duplicate channel name");
        for c in &all {
            assert!(
                c.name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                "bad channel name {}",
                c.name
            );
            assert!(!c.unit.is_empty() && !c.description.is_empty());
        }
    }
}
