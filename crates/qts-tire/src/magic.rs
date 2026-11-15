// SPDX-License-Identifier: Apache-2.0
//! Steady-state baseline: a subset of the Magic Formula.
//!
//! The equations follow MF 5.2 as given in Pacejka, *Tire and Vehicle
//! Dynamics*, with the pressure terms of MF 6.1, restricted to the
//! coefficients in [`MagicFormulaParams`](crate::params::MagicFormulaParams).
//! Shifts, ply-steer, conicity and turn slip are not implemented.
//!
//! Sign convention: positive slip ratio and positive slip angle produce
//! positive `Fx` and `Fy`. The aligning moment is `Mz = −t · Fy`, so it
//! opposes the slip angle.

use crate::params::{SurfaceParams, TireParams};
use qts_core::math::{clamp, fm, sign};

/// The basic Magic Formula curve
/// `y = D · sin(C · atan(B·x − E · (B·x − atan(B·x))))`.
#[inline]
pub fn magic_formula(b: f64, c: f64, d: f64, e: f64, x: f64) -> f64 {
    let bx = b * x;
    d * fm::sin(c * fm::atan(bx - e * (bx - fm::atan(bx))))
}

/// The cosine form used for pneumatic trail and the combined-slip weights:
/// `y = D · cos(C · atan(B·x − E · (B·x − atan(B·x))))`.
#[inline]
pub fn magic_formula_cos(b: f64, c: f64, d: f64, e: f64, x: f64) -> f64 {
    let bx = b * x;
    d * fm::cos(c * fm::atan(bx - e * (bx - fm::atan(bx))))
}

/// Conditions at which the steady-state model is evaluated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Operating {
    /// Vertical load, N (zero or positive).
    pub fz: f64,
    /// Slip ratio.
    pub kappa: f64,
    /// Tangent of the slip angle.
    pub tan_alpha: f64,
    /// Camber: lean of the wheel top toward the tire's +y side, rad. Camber
    /// thrust acts toward +y for positive camber.
    pub camber: f64,
    /// Relative pressure deviation `(p − p_nominal) / p_nominal`.
    pub dpi: f64,
    /// Multiplier on friction from the thermal model.
    pub grip_factor: f64,
    /// Multiplier on slip and cornering stiffness from the thermal model.
    pub stiffness_factor: f64,
}

impl Operating {
    /// Nominal pressure, neutral temperature factors, no camber.
    pub fn simple(fz: f64, kappa: f64, alpha: f64) -> Self {
        Self {
            fz,
            kappa,
            tan_alpha: fm::tan(alpha),
            camber: 0.0,
            dpi: 0.0,
            grip_factor: 1.0,
            stiffness_factor: 1.0,
        }
    }
}

/// Load-dependent quantities shared by the steady-state and transient
/// layers.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Characteristics {
    /// Longitudinal slip stiffness `dFx/dκ` at zero slip, N.
    pub kx: f64,
    /// Cornering stiffness `dFy/dα` at zero slip, N/rad.
    pub ky: f64,
    /// Peak longitudinal friction coefficient.
    pub mu_x: f64,
    /// Peak lateral friction coefficient.
    pub mu_y: f64,
}

/// Result of a steady-state evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SteadyForces {
    /// Longitudinal force, N.
    pub fx: f64,
    /// Lateral force, N.
    pub fy: f64,
    /// Aligning moment about the vertical axis, N·m.
    pub mz: f64,
    /// Pneumatic trail, m.
    pub pneumatic_trail: f64,
    /// Stiffness and friction at this load.
    pub characteristics: Characteristics,
}

/// Smallest friction coefficient the model will use, to keep `B = K/(C·D)`
/// finite when load-sensitivity terms are extrapolated far from nominal.
const MIN_MU: f64 = 0.05;

/// Stiffness and peak friction at the given load and conditions.
pub fn characteristics(p: &TireParams, surface: &SurfaceParams, op: &Operating) -> Characteristics {
    let mf = &p.magic_formula;
    let pr = &p.pressure;
    let fz0 = p.vertical.nominal_load_n;
    let fz = op.fz.max(0.0);
    let dfz = (fz - fz0) / fz0;
    let dpi = op.dpi;
    let stiff = surface.stiffness_scale * op.stiffness_factor;
    let grip = surface.friction_scale * op.grip_factor;

    let kx =
        fz * (mf.pkx1 + mf.pkx2 * dfz) * fm::exp(mf.pkx3 * dfz) * (1.0 + pr.ppx1 * dpi) * stiff;
    let peak_load = mf.pky2 * (1.0 + pr.ppy2 * dpi) * fz0;
    let ky = mf.pky1
        * fz0
        * (1.0 + pr.ppy1 * dpi)
        * fm::sin(2.0 * fm::atan(fz / peak_load.max(1e-6)))
        * (1.0 - mf.pky3 * op.camber.abs())
        * stiff;

    let mu_x = ((mf.pdx1 + mf.pdx2 * dfz) * (1.0 + pr.ppx3 * dpi + pr.ppx4 * dpi * dpi))
        .max(MIN_MU)
        * grip;
    let mu_y = ((mf.pdy1 + mf.pdy2 * dfz)
        * (1.0 + pr.ppy3 * dpi + pr.ppy4 * dpi * dpi)
        * (1.0 - mf.pdy3 * op.camber * op.camber))
        .max(MIN_MU)
        * grip;

    Characteristics {
        kx: kx.max(0.0),
        ky: ky.max(0.0),
        mu_x,
        mu_y,
    }
}

/// Evaluates the steady-state model.
pub fn steady_state(p: &TireParams, surface: &SurfaceParams, op: &Operating) -> SteadyForces {
    let ch = characteristics(p, surface, op);
    let fz = op.fz.max(0.0);
    if fz <= 0.0 {
        return SteadyForces {
            characteristics: ch,
            ..SteadyForces::default()
        };
    }
    let mf = &p.magic_formula;
    let fz0 = p.vertical.nominal_load_n;
    let dfz = (fz - fz0) / fz0;
    let kappa = op.kappa;
    let ta = op.tan_alpha;

    // A loose surface flattens the curve after the peak: C moves toward 1,
    // where the sine never comes back down. B is recomputed from K so the
    // initial slope is unchanged.
    let cx = 1.0 + (mf.pcx1 - 1.0) * surface.falloff;
    let cy = 1.0 + (mf.pcy1 - 1.0) * surface.falloff;

    // Pure longitudinal slip.
    let dx = ch.mu_x * fz;
    let ex = (mf.pex1 + mf.pex2 * dfz).min(1.0);
    let bx = ch.kx / (cx * dx).max(1e-9);
    let fx0 = magic_formula(bx, cx, dx, ex, kappa);

    // Pure lateral slip, plus camber thrust as a vertical shift.
    let dy = ch.mu_y * fz;
    let ey = (mf.pey1 + mf.pey2 * dfz).min(1.0);
    let by = ch.ky / (cy * dy).max(1e-9);
    let fy0_slip = magic_formula(by, cy, dy, ey, ta);
    let camber_thrust = fz * mf.pvy3 * op.camber * surface.friction_scale * op.grip_factor;

    // Combined slip: cosine weighting functions.
    let bxa = mf.rbx1 * fm::cos(fm::atan(mf.rbx2 * kappa));
    let gxa = fm::cos(mf.rcx1 * fm::atan(bxa * ta)).max(0.0);
    let byk = mf.rby1 * fm::cos(fm::atan(mf.rby2 * (ta - mf.rby3)));
    let gyk = fm::cos(mf.rcy1 * fm::atan(byk * kappa)).max(0.0);

    let fx = gxa * fx0;
    let fy_slip = gyk * fy0_slip;
    let fy = fy_slip + gyk * camber_thrust;

    // Aligning moment through pneumatic trail, using an equivalent slip
    // that includes the longitudinal slip.
    let ratio = if ch.ky > 1e-9 { ch.kx / ch.ky } else { 0.0 };
    let eq = fm::sqrt(ta * ta + ratio * ratio * kappa * kappa) * if ta < 0.0 { -1.0 } else { 1.0 };
    let bt = (mf.qbz1 + mf.qbz2 * dfz).max(0.0);
    let dt = fz * (mf.qdz1 + mf.qdz2 * dfz) * (p.geometry.unloaded_radius_m / fz0);
    let et = mf.qez1.min(1.0);
    let cos_alpha = 1.0 / fm::sqrt(1.0 + ta * ta);
    let trail = magic_formula_cos(bt, mf.qcz1, dt, et, eq) * cos_alpha;
    let mz = -trail * fy_slip;

    SteadyForces {
        fx,
        fy,
        mz,
        pneumatic_trail: trail,
        characteristics: ch,
    }
}

/// Force added by material displaced on a loose surface. It acts in the
/// direction of the slip-generated force and grows with slip.
pub fn plough_force(surface: &SurfaceParams, fz: f64, kappa: f64, tan_alpha: f64) -> (f64, f64) {
    if surface.plough_coefficient <= 0.0 || fz <= 0.0 {
        return (0.0, 0.0);
    }
    let sin_alpha = tan_alpha / fm::sqrt(1.0 + tan_alpha * tan_alpha);
    let k = surface.plough_coefficient * fz;
    (k * clamp(kappa, -1.0, 1.0), k * sin_alpha)
}

/// Slip ratio at which the pure longitudinal force peaks, found by a
/// bracketed search. Useful for driver models and tests.
pub fn peak_slip_ratio(p: &TireParams, surface: &SurfaceParams, fz: f64) -> f64 {
    let mut best = (0.0, 0.0);
    let mut k = 0.005;
    while k <= 1.0 {
        let f = steady_state(p, surface, &Operating::simple(fz, k, 0.0)).fx;
        if f > best.1 {
            best = (k, f);
        }
        k += 0.005;
    }
    best.0
}

/// Returns `sign(x)` with zero mapped to +1. Kept here so sign handling in
/// the tire model is in one place.
#[inline]
pub fn sign_nonzero(x: f64) -> f64 {
    let s = sign(x);
    if s == 0.0 {
        1.0
    } else {
        s
    }
}
