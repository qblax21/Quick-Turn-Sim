// SPDX-License-Identifier: Apache-2.0
//! Inflation pressure rising with temperature in use, and what it does to
//! the contact (D-080).

use qts_tire::magic::{steady_state, Operating};
use qts_tire::model::{step, vertical_stiffness, TireInput, TireState};
use qts_tire::test_support::{tarmac, tire};

const DT: f64 = 0.001;

fn cornering(surface: &qts_tire::SurfaceParams, slip_angle: f64) -> TireInput<'_> {
    let vx = 25.0;
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
fn hard_running_heats_the_carcass_and_the_gas_and_raises_the_pressure() {
    let p = tire();
    let s = tarmac();
    let mut st = TireState::new(p.pressure.cold_temp_c);
    let cold = st.thermal.pressure_pa(&p);
    assert!((cold - p.pressure.cold_pressure_pa).abs() < 1e-6);
    // Two minutes of hard cornering.
    for _ in 0..120_000 {
        step(&p, &mut st, &cornering(&s, 0.08), DT);
    }
    let t = st.thermal;
    assert!(
        t.carcass_c > p.pressure.cold_temp_c + 10.0,
        "carcass {:.1}",
        t.carcass_c
    );
    assert!(t.gas_c > p.pressure.cold_temp_c + 5.0, "gas {:.1}", t.gas_c);
    // The gas follows the carcass, not the other way round.
    assert!(t.gas_c < t.carcass_c);
    let hot = t.pressure_pa(&p);
    // Constant volume: hot over cold absolute pressure is the ratio of
    // absolute gas temperatures.
    let expected = (t.gas_c + 273.15) / (p.pressure.cold_temp_c + 273.15);
    assert!((hot / cold - expected).abs() < 1e-12);
    assert!(
        hot > cold * 1.015,
        "only {:.1} kPa more",
        (hot - cold) / 1000.0
    );
}

#[test]
fn hot_pressure_stiffens_the_tire_and_shortens_the_relaxation_length() {
    let p = tire();
    let s = tarmac();
    let cold = TireState::new(p.pressure.cold_temp_c);
    let mut hot = cold;
    // Same tread and carcass temperature, only the gas hotter: the
    // difference is the pressure alone.
    hot.thermal.gas_c = p.pressure.cold_temp_c + 40.0;
    let dpi_cold = cold.thermal.dpi(&p);
    let dpi_hot = hot.thermal.dpi(&p);
    assert!(dpi_hot > dpi_cold);
    assert!(vertical_stiffness(&p, &hot) > vertical_stiffness(&p, &cold));

    let once = |state: TireState| {
        let mut st = state;
        step(&p, &mut st, &cornering(&s, 0.02), DT)
    };
    let (c, h) = (once(cold), once(hot));
    // The pressure reaches the contact exactly as the Magic Formula's
    // pressure terms say: the characteristics at each pressure are those
    // of the steady-state model at that pressure, and they differ.
    let at = |state: &TireState| {
        let op = Operating {
            fz: 4000.0,
            kappa: 0.0,
            tan_alpha: 0.0,
            camber: 0.0,
            dpi: state.thermal.dpi(&p),
            grip_factor: state.thermal.grip_factor(&p.thermal),
            stiffness_factor: state.thermal.stiffness_factor(&p.thermal),
        };
        steady_state(&p, &s, &op).characteristics
    };
    for (out, state) in [(&c, &cold), (&h, &hot)] {
        let expect = at(state);
        assert!((out.characteristics.ky - expect.ky).abs() < 1e-6 * expect.ky);
        assert!((out.characteristics.mu_y - expect.mu_y).abs() < 1e-12);
    }
    assert!((h.characteristics.ky / c.characteristics.ky - 1.0).abs() > 0.005);
    // The carcass stiffens with pressure, so the contact responds over a
    // shorter rolling distance.
    assert!(p.pressure.carcass_stiffness_sensitivity > 0.0);
    assert!(h.relaxation_lat_m < c.relaxation_lat_m);
    // Peak friction falls with pressure (ppy3, ppx3 negative here).
    assert!(p.pressure.ppy3 < 0.0 && p.pressure.ppx3 < 0.0);
    assert!(h.characteristics.mu_y < c.characteristics.mu_y);
    assert!(h.characteristics.mu_x < c.characteristics.mu_x);
}
