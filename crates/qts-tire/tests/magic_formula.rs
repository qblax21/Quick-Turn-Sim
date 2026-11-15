// SPDX-License-Identifier: Apache-2.0
//! Steady-state tire model against known Magic Formula outputs.
//!
//! Reference numbers come from `tools/mf_reference.ps1`, a separate
//! implementation of the same published equations. The analytic tests check
//! properties that hold for any coefficient set.

// Reference values are pasted at the full precision the script printed.
#![allow(clippy::excessive_precision, clippy::inconsistent_digit_grouping)]

use qts_tire::magic::{magic_formula, peak_slip_ratio, steady_state, Operating};
use qts_tire::test_support::{gravel, tarmac, tire};

fn rel_close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

#[test]
fn basic_curve_matches_reference_values() {
    // B = 10, C = 1.9, D = 1, E = 0.97.
    let cases = [
        (0.05, 0.735_619_337_570_726_84),
        (0.1, 0.955_842_103_084_141_19),
        (0.2, 0.999_177_735_641_691_54),
        (0.5, 0.959_374_724_159_496),
        (1.0, 0.914_521_958_012_804_67),
    ];
    for (x, expected) in cases {
        let got = magic_formula(10.0, 1.9, 1.0, 0.97, x);
        assert!((got - expected).abs() < 1e-13, "x={x}: {got} vs {expected}");
    }
}

#[test]
fn full_model_matches_reference_values() {
    // (Fz, kappa, alpha_deg, camber_deg, dpi) -> (Fx, Fy, Mz, Kx, Ky)
    #[rustfmt::skip]
    let cases = [
        ((4000.0, 0.1, 0.0, 0.0, 0.0),
         (4259.433_946_699_76, 0.0, 0.0, 80_000.0, 57_599.999_999_999_993)),
        ((6000.0, 0.0, 4.0, 0.0, 0.0),
         (0.0, 4026.207_588_321_416_6, -132.271_520_799_605_3, 132_620.510_169_077_72, 69_120.0)),
        ((3000.0, -0.05, -6.0, 2.0, 0.0),
         (-1554.038_803_808_757_2, -2670.208_733_076_511_6, 26.039_652_149_586_09, 57_073.765_470_042_839, 46_846.696_611_378_706)),
        ((5000.0, 0.08, 3.0, 0.0, 0.1),
         (4433.253_691_445_957, 2372.730_868_756_274, -31.255_135_844_677_646, 101_973.296_348_474_34, 63_428.134_510_726_974)),
        ((2000.0, 0.3, 12.0, -1.5, -0.15),
         (1693.312_400_696_943_1, 1138.526_660_819_818_1, 0.258_624_743_996_851_59, 37_822.204_073_903_107, 38_038.419_641_128_145)),
    ];
    let p = tire();
    let s = tarmac();
    for ((fz, kappa, alpha_deg, camber_deg, dpi), (fx, fy, mz, kx, ky)) in cases {
        let op = Operating {
            fz,
            kappa,
            tan_alpha: f64::to_radians(alpha_deg).tan(),
            camber: f64::to_radians(camber_deg),
            dpi,
            grip_factor: 1.0,
            stiffness_factor: 1.0,
        };
        let got = steady_state(&p, &s, &op);
        let label =
            format!("Fz={fz} kappa={kappa} alpha={alpha_deg} camber={camber_deg} dpi={dpi}");
        assert!(
            rel_close(got.fx, fx, 1e-10),
            "{label}: Fx {} vs {fx}",
            got.fx
        );
        assert!(
            rel_close(got.fy, fy, 1e-10),
            "{label}: Fy {} vs {fy}",
            got.fy
        );
        assert!(
            rel_close(got.mz, mz, 1e-10),
            "{label}: Mz {} vs {mz}",
            got.mz
        );
        assert!(rel_close(got.characteristics.kx, kx, 1e-12), "{label}: Kx");
        assert!(rel_close(got.characteristics.ky, ky, 1e-12), "{label}: Ky");
    }
}

#[test]
fn peak_force_equals_friction_times_load() {
    let p = tire();
    let s = tarmac();
    for fz in [2000.0, 4000.0, 7000.0] {
        let ch = steady_state(&p, &s, &Operating::simple(fz, 0.0, 0.0)).characteristics;
        let mut peak_x: f64 = 0.0;
        let mut peak_y: f64 = 0.0;
        for i in 1..4000 {
            let slip = i as f64 * 0.0005;
            peak_x = peak_x.max(steady_state(&p, &s, &Operating::simple(fz, slip, 0.0)).fx);
            peak_y = peak_y.max(steady_state(&p, &s, &Operating::simple(fz, 0.0, slip)).fy);
        }
        assert!(rel_close(peak_x, ch.mu_x * fz, 1e-4), "Fx peak at {fz} N");
        assert!(rel_close(peak_y, ch.mu_y * fz, 1e-4), "Fy peak at {fz} N");
    }
}

#[test]
fn initial_slope_equals_stiffness() {
    let p = tire();
    let s = tarmac();
    let fz = 4500.0;
    let h = 1e-6;
    let ch = steady_state(&p, &s, &Operating::simple(fz, 0.0, 0.0)).characteristics;
    let slope_x = steady_state(&p, &s, &Operating::simple(fz, h, 0.0)).fx / h;
    let slope_y = steady_state(&p, &s, &Operating::simple(fz, 0.0, h)).fy / h;
    assert!(rel_close(slope_x, ch.kx, 1e-6));
    assert!(rel_close(slope_y, ch.ky, 1e-6));
}

#[test]
fn forces_are_odd_in_slip_without_camber() {
    let p = tire();
    let s = tarmac();
    for (k, a) in [(0.07, 0.0), (0.0, 0.09), (0.12, -0.05)] {
        let pos = steady_state(&p, &s, &Operating::simple(4000.0, k, a));
        let neg = steady_state(&p, &s, &Operating::simple(4000.0, -k, -a));
        assert!((pos.fx + neg.fx).abs() < 1e-9);
        assert!((pos.fy + neg.fy).abs() < 1e-9);
        assert!((pos.mz + neg.mz).abs() < 1e-9);
    }
}

#[test]
fn combined_slip_only_reduces_each_force() {
    let p = tire();
    let s = tarmac();
    let fz = 4000.0;
    let pure_x = steady_state(&p, &s, &Operating::simple(fz, 0.08, 0.0)).fx;
    let pure_y = steady_state(&p, &s, &Operating::simple(fz, 0.0, 0.08)).fy;
    let both = steady_state(&p, &s, &Operating::simple(fz, 0.08, 0.08));
    assert!(both.fx > 0.0 && both.fx < pure_x);
    assert!(both.fy > 0.0 && both.fy < pure_y);
    // The resultant stays inside a generous friction bound.
    let ch = both.characteristics;
    let limit = 1.05 * ch.mu_x.max(ch.mu_y) * fz;
    assert!((both.fx * both.fx + both.fy * both.fy).sqrt() < limit);
}

#[test]
fn friction_coefficient_falls_with_load() {
    let p = tire();
    let s = tarmac();
    let light = steady_state(&p, &s, &Operating::simple(2000.0, 0.0, 0.0)).characteristics;
    let heavy = steady_state(&p, &s, &Operating::simple(7000.0, 0.0, 0.0)).characteristics;
    assert!(heavy.mu_x < light.mu_x);
    assert!(heavy.mu_y < light.mu_y);
}

#[test]
fn aligning_moment_opposes_slip_and_fades_past_the_peak() {
    let p = tire();
    let s = tarmac();
    let small = steady_state(&p, &s, &Operating::simple(4000.0, 0.0, 0.03));
    let large = steady_state(&p, &s, &Operating::simple(4000.0, 0.0, 0.25));
    assert!(small.fy > 0.0 && small.mz < 0.0);
    assert!(small.pneumatic_trail > 0.0);
    assert!(large.pneumatic_trail < small.pneumatic_trail);
    assert!(large.mz.abs() < small.mz.abs());
}

#[test]
fn camber_produces_thrust_toward_the_lean() {
    let p = tire();
    let s = tarmac();
    let mut op = Operating::simple(4000.0, 0.0, 0.0);
    op.camber = 0.05;
    assert!(steady_state(&p, &s, &op).fy > 0.0);
    op.camber = -0.05;
    assert!(steady_state(&p, &s, &op).fy < 0.0);
}

#[test]
fn zero_load_gives_zero_force() {
    let f = steady_state(&tire(), &tarmac(), &Operating::simple(0.0, 0.2, 0.2));
    assert_eq!((f.fx, f.fy, f.mz), (0.0, 0.0, 0.0));
}

#[test]
fn loose_surface_has_lower_later_flatter_peak() {
    let p = tire();
    let hard = tarmac();
    let loose = gravel();
    let fz = 4000.0;
    let peak_hard = peak_slip_ratio(&p, &hard, fz);
    let peak_loose = peak_slip_ratio(&p, &loose, fz);
    let f_hard = steady_state(&p, &hard, &Operating::simple(fz, peak_hard, 0.0)).fx;
    let f_loose = steady_state(&p, &loose, &Operating::simple(fz, peak_loose, 0.0)).fx;
    assert!(
        f_loose < 0.75 * f_hard,
        "loose peak should be clearly lower"
    );
    assert!(
        peak_loose > peak_hard,
        "loose peak should come at more slip"
    );
    // On tarmac the force drops well below its peak when locked; on the
    // loose surface it holds up.
    let locked_hard = steady_state(&p, &hard, &Operating::simple(fz, 1.0, 0.0)).fx;
    let locked_loose = steady_state(&p, &loose, &Operating::simple(fz, 1.0, 0.0)).fx;
    assert!(locked_hard / f_hard < 0.9);
    assert!(locked_loose / f_loose > 0.93);
}
