// SPDX-License-Identifier: Apache-2.0
//! Transient, thermal and pressure behaviour of the tire model.

use qts_core::snapshot::{restore_bytes, snapshot_bytes};
use qts_tire::magic::{steady_state, Operating};
use qts_tire::model::{step, vertical_load, vertical_stiffness, TireInput, TireState};
use qts_tire::test_support::{gravel, tarmac, tire};
use qts_tire::thermal::{ThermalInput, ThermalState};

const DT: f64 = 0.001;

fn input(
    surface: &qts_tire::SurfaceParams,
    vx: f64,
    vy: f64,
    omega: f64,
    fz: f64,
) -> TireInput<'_> {
    TireInput {
        vx,
        vy,
        omega,
        fz,
        rolling_radius_m: 0.3,
        camber: 0.0,
        surface,
        air_temp_c: 20.0,
        road_temp_c: 25.0,
    }
}

#[test]
fn lateral_force_builds_over_one_relaxation_length() {
    let p = tire();
    let s = tarmac();
    let vx = 20.0;
    let alpha: f64 = 0.01; // well inside the linear range
    let inp = input(&s, vx, -vx * alpha.tan(), vx / 0.3, 4000.0);

    // Steady-state value for these conditions.
    let mut probe = TireState::new(40.0);
    let mut out = step(&p, &mut probe, &inp, DT);
    let sigma = out.relaxation_lat_m;
    for _ in 0..5000 {
        out = step(&p, &mut probe, &inp, DT);
    }
    let fy_final = out.fy;
    assert!(fy_final > 0.0);

    let mut st = TireState::new(40.0);
    let mut travelled = 0.0;
    loop {
        let o = step(&p, &mut st, &inp, DT);
        travelled += vx * DT;
        if o.fy >= 0.632_120_558_8 * fy_final {
            break;
        }
        assert!(travelled < 5.0, "force never built up");
    }
    let err = (travelled - sigma).abs() / sigma;
    assert!(
        err < 0.06,
        "63% at {travelled:.3} m, relaxation length {sigma:.3} m"
    );
}

#[test]
fn transient_settles_on_the_steady_state_model() {
    let p = tire();
    let s = tarmac();
    let mut st = TireState::new(60.0);
    let vx = 30.0;
    let kappa = 0.04;
    let alpha: f64 = 0.03;
    let omega = vx * (1.0 + kappa) / 0.3;
    let inp = input(&s, vx, -vx * alpha.tan(), omega, 4500.0);
    let mut out = step(&p, &mut st, &inp, DT);
    for _ in 0..3000 {
        out = step(&p, &mut st, &inp, DT);
    }
    let op = Operating {
        fz: 4500.0,
        kappa,
        tan_alpha: alpha.tan(),
        camber: 0.0,
        dpi: st.thermal.dpi(&p),
        grip_factor: st.thermal.grip_factor(&p.thermal),
        stiffness_factor: st.thermal.stiffness_factor(&p.thermal),
    };
    let ss = steady_state(&p, &s, &op);
    assert!(
        (out.fx - ss.fx).abs() < 1e-3 * ss.fx.abs(),
        "{} vs {}",
        out.fx,
        ss.fx
    );
    assert!(
        (out.fy - ss.fy).abs() < 1e-3 * ss.fy.abs(),
        "{} vs {}",
        out.fy,
        ss.fy
    );
    assert!((out.slip_ratio - kappa).abs() < 1e-5);
    assert!((out.slip_angle - alpha).abs() < 1e-5);
}

#[test]
fn standstill_tire_is_a_spring_not_a_source() {
    let p = tire();
    let s = tarmac();
    // Gas at the cold-inflation temperature, so pressure is nominal and the
    // carcass stiffness is exactly the parameter value.
    let mut st = TireState::new(p.pressure.cold_temp_c);
    // At rest with no motion the tire makes no force, indefinitely.
    for _ in 0..1000 {
        let o = step(&p, &mut st, &input(&s, 0.0, 0.0, 0.0, 4000.0), DT);
        assert_eq!((o.fx, o.fy), (0.0, 0.0));
    }
    // Push the contact patch 1 mm sideways slowly, then stop.
    for _ in 0..1000 {
        step(&p, &mut st, &input(&s, 0.0, 0.001, 0.0, 4000.0), DT);
    }
    let held = step(&p, &mut st, &input(&s, 0.0, 0.0, 0.0, 4000.0), DT);
    // The restoring force is the carcass stiffness times the displacement,
    // and it opposes the push.
    let carcass = p.transient.carcass_stiffness_lat_n_per_m;
    assert!(held.fy < 0.0);
    assert!(
        (held.fy + carcass * 0.001).abs() < 0.03 * carcass * 0.001,
        "fy={}",
        held.fy
    );
}

#[test]
fn spinning_wheel_at_standstill_saturates_without_windup() {
    let p = tire();
    let s = tarmac();
    let mut st = TireState::new(40.0);
    let mut out = step(&p, &mut st, &input(&s, 0.0, 0.0, 50.0, 4000.0), DT);
    for _ in 0..2000 {
        out = step(&p, &mut st, &input(&s, 0.0, 0.0, 50.0, 4000.0), DT);
    }
    assert!(out.fx > 0.0 && out.fx.is_finite());
    assert!(out.slip_ratio <= p.transient.slip_ratio_limit + 1e-12);
    let limit = out.relaxation_long_m * p.transient.slip_ratio_limit;
    assert!(st.deflection_long_m.abs() <= limit + 1e-12);
}

#[test]
fn airborne_tire_makes_no_force_and_relaxes() {
    let p = tire();
    let s = tarmac();
    let mut st = TireState::new(40.0);
    st.deflection_lat_m = 0.01;
    let o = step(&p, &mut st, &input(&s, 10.0, 2.0, 0.0, 0.0), DT);
    assert_eq!((o.fx, o.fy, o.mz), (0.0, 0.0, 0.0));
    assert_eq!(st.deflection_lat_m, 0.0);
}

#[test]
fn loose_surface_adds_rolling_resistance_and_plough_force() {
    let p = tire();
    let hard = tarmac();
    let loose = gravel();
    let mut a = TireState::new(40.0);
    let mut b = TireState::new(40.0);
    let free_hard = step(&p, &mut a, &input(&hard, 20.0, 0.0, 20.0 / 0.3, 4000.0), DT);
    let free_loose = step(
        &p,
        &mut b,
        &input(&loose, 20.0, 0.0, 20.0 / 0.3, 4000.0),
        DT,
    );
    assert!(free_loose.rolling_resistance_torque > 2.0 * free_hard.rolling_resistance_torque);
    // Locked wheel: the loose surface makes more retarding force than its
    // Magic Formula plateau alone because of ploughing.
    let mut c = TireState::new(40.0);
    let mut locked = step(&p, &mut c, &input(&loose, 20.0, 0.0, 0.0, 4000.0), DT);
    for _ in 0..2000 {
        locked = step(&p, &mut c, &input(&loose, 20.0, 0.0, 0.0, 4000.0), DT);
    }
    let mf_only = steady_state(
        &p,
        &loose,
        &Operating {
            fz: 4000.0,
            kappa: -1.0,
            tan_alpha: 0.0,
            camber: 0.0,
            dpi: c.thermal.dpi(&p),
            grip_factor: c.thermal.grip_factor(&p.thermal),
            stiffness_factor: c.thermal.stiffness_factor(&p.thermal),
        },
    );
    assert!(locked.fx < mf_only.fx, "{} vs {}", locked.fx, mf_only.fx);
    let plough = loose.plough_coefficient * 4000.0;
    assert!((locked.fx - (mf_only.fx - plough)).abs() < 0.02 * plough);
}

#[test]
fn sliding_heats_surface_first_and_rolling_cools_it() {
    let p = tire();
    let s = tarmac();
    let mut st = TireState::new(25.0);
    // Ten seconds of a heavy slide.
    for _ in 0..10_000 {
        step(&p, &mut st, &input(&s, 25.0, -6.0, 25.0 / 0.3, 5000.0), DT);
    }
    let hot = st.thermal;
    assert!(hot.surface_c > 40.0, "surface {}", hot.surface_c);
    assert!(hot.surface_c > hot.core_c && hot.core_c > hot.carcass_c);
    assert!(hot.carcass_c >= 25.0 && hot.gas_c >= 25.0);
    // Thirty seconds of straight free rolling.
    for _ in 0..30_000 {
        step(&p, &mut st, &input(&s, 25.0, 0.0, 25.0 / 0.3, 5000.0), DT);
    }
    assert!(st.thermal.surface_c < hot.surface_c);
    assert!(st.thermal.is_finite());
}

#[test]
fn heat_is_conserved_when_nothing_can_escape() {
    let mut p = tire();
    p.thermal.conductance_surface_road_w_per_k = 0.0;
    p.thermal.convection_surface_w_per_k = 0.0;
    p.thermal.convection_surface_speed_gain = 0.0;
    p.thermal.convection_carcass_w_per_k = 0.0;
    p.thermal.convection_carcass_speed_gain = 0.0;
    let th = &p.thermal;
    let mut t = ThermalState::uniform(30.0);
    let inp = ThermalInput {
        sliding_power_w: 2000.0,
        rolling_power_w: 300.0,
        speed_m_per_s: 30.0,
        in_contact: true,
        air_temp_c: 20.0,
        road_temp_c: 25.0,
    };
    let steps = 20_000;
    for _ in 0..steps {
        t.step(th, &inp, DT);
    }
    let stored = th.surface_heat_capacity_j_per_k * (t.surface_c - 30.0)
        + th.core_heat_capacity_j_per_k * (t.core_c - 30.0)
        + th.carcass_heat_capacity_j_per_k * (t.carcass_c - 30.0)
        + th.gas_heat_capacity_j_per_k * (t.gas_c - 30.0);
    let supplied = (2000.0 * th.friction_heat_fraction + 300.0) * steps as f64 * DT;
    assert!(
        (stored - supplied).abs() < 1e-6 * supplied,
        "{stored} vs {supplied}"
    );
}

#[test]
fn grip_window_peaks_at_the_optimum() {
    let p = tire();
    let th = &p.thermal;
    let at = |t: f64| ThermalState::uniform(t).grip_factor(th);
    assert!((at(th.optimal_temp_c) - 1.0).abs() < 1e-12);
    assert!(at(th.optimal_temp_c - 30.0) < 1.0);
    assert!(at(th.optimal_temp_c + 30.0) < 1.0);
    assert!((at(th.optimal_temp_c - 30.0) - at(th.optimal_temp_c + 30.0)).abs() < 1e-12);
    assert!(at(-200.0) >= th.min_grip_factor - 1e-12);
    assert!((at(1000.0) - th.min_grip_factor).abs() < 1e-6);
}

#[test]
fn pressure_follows_gas_temperature_and_stiffens_the_tire() {
    let p = tire();
    let cold = TireState::new(p.pressure.cold_temp_c);
    let mut hot = cold;
    hot.thermal.gas_c = p.pressure.cold_temp_c + 40.0;
    let p_cold = cold.thermal.pressure_pa(&p);
    let p_hot = hot.thermal.pressure_pa(&p);
    assert!((p_cold - p.pressure.cold_pressure_pa).abs() < 1e-6);
    // Constant volume: pressure ratio equals absolute temperature ratio.
    let expected = (p.pressure.cold_temp_c + 40.0 + 273.15) / (p.pressure.cold_temp_c + 273.15);
    assert!((p_hot / p_cold - expected).abs() < 1e-12);
    assert!(vertical_stiffness(&p, &hot) > vertical_stiffness(&p, &cold));
    assert!(vertical_load(&p, &hot, 0.02, 0.0) > vertical_load(&p, &cold, 0.02, 0.0));
    assert_eq!(vertical_load(&p, &cold, -0.01, -5.0), 0.0);
    assert_eq!(vertical_load(&p, &cold, 0.001, -1000.0), 0.0);
}

#[test]
fn tire_state_snapshot_round_trips() {
    let p = tire();
    let s = tarmac();
    let mut st = TireState::new(33.0);
    for _ in 0..500 {
        step(&p, &mut st, &input(&s, 15.0, -1.0, 52.0, 4200.0), DT);
    }
    let bytes = snapshot_bytes(&st);
    let mut back = TireState::new(0.0);
    restore_bytes(&mut back, &bytes).unwrap();
    assert_eq!(st, back);
    // Both continue identically.
    let a = step(&p, &mut st, &input(&s, 15.0, -1.0, 52.0, 4200.0), DT);
    let b = step(&p, &mut back, &input(&s, 15.0, -1.0, 52.0, 4200.0), DT);
    assert_eq!(a, b);
}
