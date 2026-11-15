// SPDX-License-Identifier: Apache-2.0
//! Small `f64` math kit used by every simulation crate.
//!
//! It is hand-written so that each floating-point operation in the
//! determinism path is visible here and nothing depends on an external
//! library's version (see `DECISIONS.md`, D-003).

use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

/// Transcendental functions used by simulation code.
///
/// All sim crates call these instead of the inherent `f64` methods. Today
/// they forward to `std`, whose results can differ between platforms. This
/// module is the single switch point for a software math library later
/// (see `ARCHITECTURE.md`, section 5).
pub mod fm {
    /// Sine.
    #[inline]
    pub fn sin(x: f64) -> f64 {
        x.sin()
    }
    /// Cosine.
    #[inline]
    pub fn cos(x: f64) -> f64 {
        x.cos()
    }
    /// Tangent.
    #[inline]
    pub fn tan(x: f64) -> f64 {
        x.tan()
    }
    /// Arc sine.
    #[inline]
    pub fn asin(x: f64) -> f64 {
        x.asin()
    }
    /// Arc tangent.
    #[inline]
    pub fn atan(x: f64) -> f64 {
        x.atan()
    }
    /// Four-quadrant arc tangent of `y / x`.
    #[inline]
    pub fn atan2(y: f64, x: f64) -> f64 {
        y.atan2(x)
    }
    /// Natural exponential.
    #[inline]
    pub fn exp(x: f64) -> f64 {
        x.exp()
    }
    /// Hyperbolic tangent.
    #[inline]
    pub fn tanh(x: f64) -> f64 {
        x.tanh()
    }
    /// `x` raised to the power `p`.
    #[inline]
    pub fn powf(x: f64, p: f64) -> f64 {
        x.powf(p)
    }
    /// Square root. IEEE-754 specifies this exactly on every platform.
    #[inline]
    pub fn sqrt(x: f64) -> f64 {
        x.sqrt()
    }
}

/// Clamps `x` to `[lo, hi]`.
#[inline]
pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

/// Linear interpolation from `a` to `b` by `t`.
#[inline]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Sign of `x` as -1, 0 or 1.
#[inline]
pub fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// Cubic smoothstep of `t` clamped to `[0, 1]`.
#[inline]
pub fn smoothstep(t: f64) -> f64 {
    let t = clamp(t, 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Piecewise-linear lookup in a table of `(x, y)` points sorted by `x`.
///
/// Values outside the table are clamped to the first or last point.
pub fn interp_table(points: &[(f64, f64)], x: f64) -> f64 {
    match points {
        [] => 0.0,
        [only] => only.1,
        _ => {
            if x <= points[0].0 {
                return points[0].1;
            }
            for pair in points.windows(2) {
                let (x0, y0) = pair[0];
                let (x1, y1) = pair[1];
                if x <= x1 {
                    let span = x1 - x0;
                    if span <= 0.0 {
                        return y1;
                    }
                    return lerp(y0, y1, (x - x0) / span);
                }
            }
            points[points.len() - 1].1
        }
    }
}

/// Three-component vector.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vec3 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl Vec3 {
    /// The zero vector.
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);
    /// Unit vector along x.
    pub const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);
    /// Unit vector along y.
    pub const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);
    /// Unit vector along z.
    pub const Z: Vec3 = Vec3::new(0.0, 0.0, 1.0);

    /// Builds a vector from components.
    #[inline]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Dot product.
    #[inline]
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    /// Cross product, right-handed.
    #[inline]
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    /// Squared length.
    #[inline]
    pub fn length_sq(self) -> f64 {
        self.dot(self)
    }

    /// Length.
    #[inline]
    pub fn length(self) -> f64 {
        fm::sqrt(self.length_sq())
    }

    /// Unit vector in the same direction, or `fallback` if the length is
    /// too small to normalise.
    #[inline]
    pub fn normalized_or(self, fallback: Vec3) -> Vec3 {
        let len = self.length();
        if len > 1e-12 {
            self * (1.0 / len)
        } else {
            fallback
        }
    }

    /// True when every component is finite.
    #[inline]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    /// Components as an array.
    #[inline]
    pub fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    #[inline]
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    #[inline]
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for Vec3 {
    type Output = Vec3;
    #[inline]
    fn mul(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    #[inline]
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}
impl AddAssign for Vec3 {
    #[inline]
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}
impl SubAssign for Vec3 {
    #[inline]
    fn sub_assign(&mut self, o: Vec3) {
        *self = *self - o;
    }
}

/// Unit quaternion representing an orientation (body frame to world frame).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    /// Scalar part.
    pub w: f64,
    /// Vector part, x.
    pub x: f64,
    /// Vector part, y.
    pub y: f64,
    /// Vector part, z.
    pub z: f64,
}

impl Default for Quat {
    fn default() -> Self {
        Quat::IDENTITY
    }
}

impl Quat {
    /// No rotation.
    pub const IDENTITY: Quat = Quat {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// Rotation of `angle` radians about a unit `axis`.
    pub fn from_axis_angle(axis: Vec3, angle: f64) -> Quat {
        let h = 0.5 * angle;
        let s = fm::sin(h);
        Quat {
            w: fm::cos(h),
            x: axis.x * s,
            y: axis.y * s,
            z: axis.z * s,
        }
    }

    /// Rotation about the world z axis (heading), radians, counter-clockwise
    /// seen from above.
    pub fn from_yaw(yaw: f64) -> Quat {
        Quat::from_axis_angle(Vec3::Z, yaw)
    }

    /// Conjugate, which is the inverse for a unit quaternion.
    #[inline]
    pub fn conjugate(self) -> Quat {
        Quat {
            w: self.w,
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }

    /// Rotates a body-frame vector into the world frame.
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let u = Vec3::new(self.x, self.y, self.z);
        let t = u.cross(v) * 2.0;
        v + t * self.w + u.cross(t)
    }

    /// Rotates a world-frame vector into the body frame.
    #[inline]
    pub fn inverse_rotate(self, v: Vec3) -> Vec3 {
        self.conjugate().rotate(v)
    }

    /// Returns the quaternion scaled to unit length.
    pub fn normalized(self) -> Quat {
        let n = fm::sqrt(self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z);
        if n > 1e-12 {
            let inv = 1.0 / n;
            Quat {
                w: self.w * inv,
                x: self.x * inv,
                y: self.y * inv,
                z: self.z * inv,
            }
        } else {
            Quat::IDENTITY
        }
    }

    /// Advances the orientation by a body-frame angular velocity over `dt`.
    ///
    /// Uses the exact rotation for a constant angular velocity over the
    /// step, then renormalises.
    pub fn integrate_body_rate(self, omega_body: Vec3, dt: f64) -> Quat {
        let angle = omega_body.length() * dt;
        if angle < 1e-14 {
            return self;
        }
        let axis = omega_body.normalized_or(Vec3::Z);
        (self * Quat::from_axis_angle(axis, angle)).normalized()
    }

    /// Heading angle about world z of the body x axis, radians.
    pub fn yaw(self) -> f64 {
        let fwd = self.rotate(Vec3::X);
        fm::atan2(fwd.y, fwd.x)
    }

    /// Pitch of the body x axis above the horizontal, radians (nose up
    /// positive).
    pub fn pitch(self) -> f64 {
        let fwd = self.rotate(Vec3::X);
        fm::asin(clamp(fwd.z, -1.0, 1.0))
    }

    /// Roll about the body x axis, radians (left side up positive).
    pub fn roll(self) -> f64 {
        let left = self.rotate(Vec3::Y);
        fm::asin(clamp(left.z, -1.0, 1.0))
    }

    /// True when every component is finite.
    pub fn is_finite(self) -> bool {
        self.w.is_finite() && self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

/// Hamilton product `a * b`: apply `b` first, then `a`.
impl Mul<Quat> for Quat {
    type Output = Quat;
    fn mul(self, o: Quat) -> Quat {
        Quat {
            w: self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
            x: self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            y: self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            z: self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
        }
    }
}

/// Solves `A x = b` in place for a symmetric positive-definite `A` by
/// Cholesky factorisation. On success `b` holds `x` and `a` holds the
/// factor. Returns `false` if `A` is not positive definite.
///
/// The elimination order is fixed, so results are reproducible.
pub fn solve_spd<const N: usize>(a: &mut [[f64; N]; N], b: &mut [f64; N]) -> bool {
    for j in 0..N {
        let mut d = a[j][j];
        for k in 0..j {
            d -= a[j][k] * a[j][k];
        }
        if d <= 0.0 || !d.is_finite() {
            return false;
        }
        let d = fm::sqrt(d);
        a[j][j] = d;
        for i in (j + 1)..N {
            let mut s = a[i][j];
            for k in 0..j {
                s -= a[i][k] * a[j][k];
            }
            a[i][j] = s / d;
        }
    }
    // Forward substitution: L y = b.
    for i in 0..N {
        let mut s = b[i];
        for k in 0..i {
            s -= a[i][k] * b[k];
        }
        b[i] = s / a[i][i];
    }
    // Back substitution: Lᵀ x = y.
    for i in (0..N).rev() {
        let mut s = b[i];
        for k in (i + 1)..N {
            s -= a[k][i] * b[k];
        }
        b[i] = s / a[i][i];
    }
    true
}

/// Stateless value noise over the plane, in `[-1, 1]`.
///
/// The value at each integer lattice point comes from an integer hash and
/// points in between are blended smoothly. It is a pure function of position,
/// which lets road roughness exist without any random state in the sim.
pub fn lattice_noise_2d(x: f64, y: f64) -> f64 {
    let xf = x.floor();
    let yf = y.floor();
    let ix = xf as i64;
    let iy = yf as i64;
    let tx = smoothstep(x - xf);
    let ty = smoothstep(y - yf);
    let v00 = lattice_value(ix, iy);
    let v10 = lattice_value(ix + 1, iy);
    let v01 = lattice_value(ix, iy + 1);
    let v11 = lattice_value(ix + 1, iy + 1);
    lerp(lerp(v00, v10, tx), lerp(v01, v11, tx), ty)
}

fn lattice_value(ix: i64, iy: i64) -> f64 {
    // SplitMix64-style integer mix of the two lattice coordinates.
    let mut h = (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    // Top 53 bits to a float in [0, 1), then to [-1, 1).
    let unit = (h >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0);
    unit * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn cross_is_right_handed() {
        assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
        assert_eq!(Vec3::Y.cross(Vec3::Z), Vec3::X);
    }

    #[test]
    fn yaw_rotation_turns_x_toward_y() {
        let q = Quat::from_yaw(std::f64::consts::FRAC_PI_2);
        let v = q.rotate(Vec3::X);
        assert!(close(v.x, 0.0, 1e-12) && close(v.y, 1.0, 1e-12));
        assert!(close(q.yaw(), std::f64::consts::FRAC_PI_2, 1e-12));
    }

    #[test]
    fn inverse_rotate_undoes_rotate() {
        let q = Quat::from_axis_angle(Vec3::new(0.3, -0.5, 0.8).normalized_or(Vec3::Z), 1.1);
        let v = Vec3::new(1.0, 2.0, -3.0);
        let back = q.inverse_rotate(q.rotate(v));
        assert!((back - v).length() < 1e-12);
    }

    #[test]
    fn integrate_body_rate_matches_axis_angle() {
        let q = Quat::IDENTITY.integrate_body_rate(Vec3::new(0.0, 0.0, 2.0), 0.25);
        assert!(close(q.yaw(), 0.5, 1e-12));
    }

    #[test]
    fn spd_solve_recovers_known_solution() {
        let mut a = [[4.0, 1.0, 0.5], [1.0, 3.0, 0.2], [0.5, 0.2, 2.0]];
        let x = [1.0, -2.0, 3.0];
        let mut b = [0.0; 3];
        for i in 0..3 {
            for j in 0..3 {
                b[i] += a[i][j] * x[j];
            }
        }
        assert!(solve_spd(&mut a, &mut b));
        for i in 0..3 {
            assert!(close(b[i], x[i], 1e-12));
        }
    }

    #[test]
    fn spd_solve_rejects_indefinite_matrix() {
        let mut a = [[1.0, 2.0], [2.0, 1.0]];
        let mut b = [1.0, 1.0];
        assert!(!solve_spd(&mut a, &mut b));
    }

    #[test]
    fn noise_is_bounded_repeatable_and_continuous() {
        for i in 0..200 {
            let x = i as f64 * 0.37 - 20.0;
            let y = i as f64 * 0.11 + 3.0;
            let n = lattice_noise_2d(x, y);
            assert!((-1.0..=1.0).contains(&n));
            assert_eq!(n, lattice_noise_2d(x, y));
            let n2 = lattice_noise_2d(x + 1e-6, y);
            assert!((n - n2).abs() < 1e-4);
        }
    }

    #[test]
    fn table_interpolates_and_clamps() {
        let t = [(0.0, 0.0), (10.0, 100.0), (20.0, 50.0)];
        assert_eq!(interp_table(&t, -5.0), 0.0);
        assert_eq!(interp_table(&t, 5.0), 50.0);
        assert_eq!(interp_table(&t, 15.0), 75.0);
        assert_eq!(interp_table(&t, 99.0), 50.0);
    }
}
