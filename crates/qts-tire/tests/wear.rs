// SPDX-License-Identifier: Apache-2.0
//! Tread wear: driven by sliding energy, and what a worn tire does.

use qts_tire::model::{step, TireInput, TireState};
use qts_tire::test_support::{tarmac, tire};
use qts_tire::thermal::{ThermalInput, ThermalState};

const DT: f64 = 0.001;

fn sliding(surface: &qts_tire::SurfaceParams, slip_angle: f64) -> TireInput<'_> {
    let vx = 20.0;
    TireInput {
        vx,
        vy: -vx * slip_angle.tan(),
        omega: vx / 0.3,
        fz: 4000.0,
        rolling_radius_m: 0.3,
        camber: 0.0,
        surface,
        air_temp_c: 20.0,
        road_temp_c: 25.0,
    }
}

#[test]
fn wear_is_the_sliding_energy_times_the_rate() {
    let p = tire();
    let s = tarmac();
    let mut state = TireState::new(60.0);
    let mut energy = 0.0;
    for _ in 0..5000 {
        let out = step(&p, &mut state, &sliding(&s, 0.12), DT);
        energy += out.sliding_power_w * DT;
    }
    let expected = p.wear.rate_per_mj * energy * 1e-6;
    assert!(energy > 1e4, "sliding energy {energy} J");
    assert!(
        (state.wear - expected).abs() < 1e-9 * expected.max(1e-12) + 1e-15,
        "wear {} expected {expected}",
        state.wear
    );
    // Rolling straight with no slip wears nothing.
    let mut straight = TireState::new(60.0);
    for _ in 0..5000 {
        step(&p, &mut straight, &sliding(&s, 0.0), DT);
    }
    assert!(straight.wear < 1e-6 * state.wear);
}

#[test]
fn a_worn_tire_grips_less_responds_sooner_and_heats_faster() {
    let p = tire();
    let s = tarmac();
    // Settled lateral force at a slip angle, with wear and temperature held.
    let settled = |wear: f64, slip: f64| {
        let mut st = TireState::new(p.thermal.optimal_temp_c);
        st.wear = wear;
        let mut fy: f64 = 0.0;
        for _ in 0..1500 {
            let mut probe = st;
            fy = step(&p, &mut probe, &sliding(&s, slip), DT).fy.abs();
            st.deflection_lat_m = probe.deflection_lat_m;
            st.deflection_long_m = probe.deflection_long_m;
        }
        fy
    };
    // Peak of the lateral force over a sweep of slip angles.
    let peak = |wear: f64| {
        (1..=40)
            .map(|i| settled(wear, 0.01 * f64::from(i)))
            .fold(0.0_f64, f64::max)
    };
    let (new, worn) = (peak(0.0), peak(1.0));
    let loss = 1.0 - worn / new;
    assert!(
        (loss - p.wear.grip_loss_at_full_wear).abs() < 0.02,
        "grip loss {loss:.3}"
    );
    let stiffness = |wear: f64| settled(wear, 0.005);
    let gain = stiffness(1.0) / stiffness(0.0) - 1.0;
    assert!(
        gain > 0.5 * p.wear.stiffness_gain_at_full_wear,
        "stiffness gain {gain:.3}"
    );

    // Same heat input: the thinner tread warms faster, and the energy it
    // stores matches what went in.
    let th = &p.thermal;
    let mut quiet = p.thermal.clone();
    quiet.conductance_surface_road_w_per_k = 0.0;
    quiet.convection_surface_w_per_k = 0.0;
    quiet.convection_surface_speed_gain = 0.0;
    quiet.convection_carcass_w_per_k = 0.0;
    quiet.convection_carcass_speed_gain = 0.0;
    let heat = |tread: f64| {
        let mut t = ThermalState::uniform(30.0);
        let inp = ThermalInput {
            sliding_power_w: 2000.0,
            rolling_power_w: 0.0,
            speed_m_per_s: 20.0,
            in_contact: true,
            air_temp_c: 20.0,
            road_temp_c: 25.0,
            tread_scale: tread,
        };
        for _ in 0..10_000 {
            t.step(&quiet, &inp, DT);
        }
        t
    };
    let (fresh, thin) = (heat(1.0), heat(0.5));
    assert!(thin.surface_c > fresh.surface_c);
    assert!(
        thin.core_c - 30.0 > 1.5 * (fresh.core_c - 30.0),
        "core rise {:.2} K worn, {:.2} K new",
        thin.core_c - 30.0,
        fresh.core_c - 30.0
    );
    let stored = |t: &ThermalState, tread: f64| {
        tread * th.surface_heat_capacity_j_per_k * (t.surface_c - 30.0)
            + tread * th.core_heat_capacity_j_per_k * (t.core_c - 30.0)
            + th.carcass_heat_capacity_j_per_k * (t.carcass_c - 30.0)
            + th.gas_heat_capacity_j_per_k * (t.gas_c - 30.0)
    };
    let input = 2000.0 * th.friction_heat_fraction * 10.0;
    for (t, tread) in [(fresh, 1.0), (thin, 0.5)] {
        let s = stored(&t, tread);
        assert!((s - input).abs() < 1e-6 * input, "stored {s} of {input}");
    }
}
