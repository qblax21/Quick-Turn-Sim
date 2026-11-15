// SPDX-License-Identifier: Apache-2.0
//! What the vehicle needs to know about the world under it.

use qts_tire::SurfaceParams;

/// Ground properties at one point.
#[derive(Clone, Copy, Debug)]
pub struct GroundSample<'a> {
    /// Height of the ground above the world origin, m.
    pub height: f64,
    /// Surface at this point.
    pub surface: &'a SurfaceParams,
    /// Index of the surface in the session's surface table.
    pub surface_index: u16,
}

/// A queryable ground. Implementations must be pure functions of position:
/// the same `(x, y)` always returns the same sample.
pub trait Ground {
    /// Ground properties at world position `(x, y)`.
    fn sample(&self, x: f64, y: f64) -> GroundSample<'_>;
}

/// An infinite flat plane of one surface at height zero.
#[derive(Clone, Debug)]
pub struct FlatGround {
    /// The surface everywhere.
    pub surface: SurfaceParams,
}

impl Ground for FlatGround {
    fn sample(&self, _x: f64, _y: f64) -> GroundSample<'_> {
        GroundSample {
            height: 0.0,
            surface: &self.surface,
            surface_index: 0,
        }
    }
}
