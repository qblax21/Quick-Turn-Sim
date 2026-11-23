// SPDX-License-Identifier: Apache-2.0
//! Tracks: surface map, paths, cones and timing gates.
//!
//! A track file describes a flat world: a default surface, regions that
//! override it (rectangles, circles, and strips along a path), cones, and
//! timing gates. A track may also carry a `terrain` section: an elevation
//! and banking profile along one path, which shapes the road exactly and
//! blends into smooth terrain away from it (see `DECISIONS.md`, D-049).
//! Without it the ground is flat apart from surface roughness and the ridge
//! profile a region may carry (used for kerbs).
//!
//! Everything here is a pure function of position, as the determinism rules
//! require: no caches that depend on query order.

use qts_core::math::{fm, lattice_noise_2d, smoothstep, Vec3};
use qts_tire::SurfaceParams;
use qts_vehicle::{Ground, GroundSample};
use serde::Deserialize;

/// Spacing of points along a generated path, m.
const PATH_STEP_M: f64 = 1.0;
/// Largest heading change between path points on an arc, rad.
const PATH_STEP_RAD: f64 = 0.05;
/// Path segments per bounding-box chunk.
const CHUNK: usize = 16;
/// Radius of a cone base, m.
pub const CONE_RADIUS_M: f64 = 0.14;
/// Half the thickness of a barrier, m.
pub const WALL_RADIUS_M: f64 = 0.25;
/// Trunk radius of a tree of size 1, m.
const TRUNK_RADIUS_M: f64 = 0.22;
/// Length of one straight piece of a barrier, m.
const WALL_STEP_M: f64 = 4.0;

/// A track file, as written in `data/tracks`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TrackDef {
    /// Identifier, equal to the file stem.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Free-text description.
    #[serde(default)]
    pub description: String,
    /// Surface everywhere no region applies.
    pub default_surface: String,
    /// Air temperature for the session, °C.
    pub air_temp_c: f64,
    /// Road surface temperature for the session, °C.
    pub road_temp_c: f64,
    /// Half-size of the square of ground worth drawing, m.
    pub ground_half_extent_m: f64,
    /// Where the car starts.
    pub spawn: SpawnDef,
    /// Paths built from straights and arcs.
    #[serde(default)]
    pub paths: Vec<PathDef>,
    /// Surface regions; later entries win where they overlap.
    #[serde(default)]
    pub regions: Vec<RegionDef>,
    /// Individually placed cones.
    #[serde(default)]
    pub cones: Vec<ConeDef>,
    /// Cones placed in pairs along a path.
    #[serde(default)]
    pub cone_pairs: Vec<ConePairsDef>,
    /// Timing gates.
    #[serde(default)]
    pub gates: Vec<GateDef>,
    /// Painted markings, for drawing only.
    #[serde(default)]
    pub markers: Vec<MarkerDef>,
    /// Elevation and banking along one path.
    #[serde(default)]
    pub terrain: Option<TerrainDef>,
    /// Solid trees scattered beside the paths.
    #[serde(default)]
    pub trees: Option<TreesDef>,
    /// Solid barriers running beside a path.
    #[serde(default)]
    pub barriers: Vec<BarrierDef>,
    /// Track limits: the road a lap must stay on.
    #[serde(default)]
    pub track_limits: Option<TrackLimitsDef>,
    /// Drift judging zones, on a track laid out for drifting (D-114).
    #[serde(default)]
    pub drift: Option<DriftDef>,
}

/// Drift judging: one zone per judged corner along a path.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DriftDef {
    /// Path the zones are measured along.
    pub path: String,
    /// Zones in lap order.
    pub zones: Vec<DriftZoneDef>,
}

/// One judged corner. Offsets are across the path, positive to the left.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DriftZoneDef {
    /// Name shown in results, such as "T3".
    pub name: String,
    /// Start of the zone along the path, m.
    pub from_m: f64,
    /// End of the zone along the path, m.
    pub to_m: f64,
    /// Inner clipping point: distance along the path, m.
    pub icp_at_m: f64,
    /// Inner clipping point: offset across the path, m.
    pub icp_offset_m: f64,
    /// Outer clipping zone: start along the path, m.
    pub ocz_from_m: f64,
    /// Outer clipping zone: end along the path, m.
    pub ocz_to_m: f64,
    /// Outer clipping zone: offset of its line across the path, m.
    pub ocz_offset_m: f64,
}

/// The road a lap must stay on: within `half_width_m` of a path, or on a
/// kerb. A lap with all four wheels off it does not count.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TrackLimitsDef {
    /// Path the road follows.
    pub path: String,
    /// Half width of the road either side of the path, m. Kerbs beside it
    /// also count as track.
    pub half_width_m: f64,
}

/// Trees scattered beside every path from a fixed seed, so a track always
/// has the same ones. Each is a solid trunk.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TreesDef {
    /// Seed of the placement.
    pub seed: u64,
    /// Distance between the points along a path that trees are placed from, m.
    pub spacing_m: f64,
    /// Trees tried at each of those points.
    pub per_point: u32,
    /// No tree stands closer than this to any path, m.
    pub keep_clear_m: f64,
    /// No tree stands farther than this from the path it was placed from, m.
    pub reach_m: f64,
}

/// A barrier at a fixed offset from part of a path.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BarrierDef {
    /// Path name.
    pub path: String,
    /// Start, distance along the path, m.
    pub from_m: f64,
    /// End, distance along the path, m.
    pub to_m: f64,
    /// Offset from the path, m; positive is to the left of its direction.
    pub offset_m: f64,
}

/// A solid tree: a trunk the car cannot pass through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tree {
    /// Position.
    pub pos: [f64; 2],
    /// Radius of the trunk, m.
    pub trunk_radius: f64,
    /// Size relative to a standard tree, for drawing.
    pub size: f64,
}

/// One straight piece of a barrier, solid out to [`WALL_RADIUS_M`] from the
/// line between its ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wall {
    /// One end.
    pub a: [f64; 2],
    /// The other end.
    pub b: [f64; 2],
}

/// Elevation and banking of the ground, defined along one path.
///
/// Within `corridor_half_width_m` of the path the ground is the road
/// surface: the profile height at that distance along the path, tilted
/// across the path by the banking angle. Over the next `blend_m` it blends
/// into smooth terrain interpolated from the elevation points; beyond that
/// it is that terrain alone. Parts of the path that are not neighbours must
/// stay further apart than twice `corridor_half_width_m + blend_m`, or the
/// ground between them has a step.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TerrainDef {
    /// Path the profile runs along.
    pub path: String,
    /// Half width of the exactly shaped strip, m.
    pub corridor_half_width_m: f64,
    /// Width of the blend from that strip into the terrain, m.
    pub blend_m: f64,
    /// Height along the path, in increasing order of distance. The profile
    /// is level at its first and last point.
    pub elevation: Vec<ElevationPoint>,
    /// Banking along the path, in increasing order of distance.
    #[serde(default)]
    pub banking: Vec<BankingPoint>,
}

/// One point of an elevation profile.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ElevationPoint {
    /// Distance along the path, m.
    pub at_m: f64,
    /// Height of the path centre line, m.
    pub height_m: f64,
}

/// One point of a banking profile.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BankingPoint {
    /// Distance along the path, m.
    pub at_m: f64,
    /// Cross slope, degrees. Positive raises the left edge of the path.
    pub angle_deg: f64,
}

/// Smoothing length of the terrain interpolation, m.
const TERRAIN_SMOOTHING_M: f64 = 40.0;
/// Step used to measure the ground slope, m.
const SLOPE_STEP_M: f64 = 0.25;
/// Width over which a tire averages the slope of a kerb, m: about a
/// tire's tread width. A tire does not feel the sharp edge of a kerb ramp
/// at a point; its tread spans it, so the contact plane it sees tilts
/// progressively as it climbs on.
pub const KERB_ENVELOPE_M: f64 = 0.2;
/// Length over which a kerb's height tapers to nothing at each end, m.
/// Real kerbs start and stop with a sloped nose rather than a step.
pub const KERB_END_TAPER_M: f64 = 1.0;

/// Smooth curve through `(distance, value)` points: a cubic Hermite spline
/// that stays between its points, level at both ends and constant beyond
/// them.
fn profile(points: &[(f64, f64)], s: f64) -> f64 {
    let Some(&(first_s, first_v)) = points.first() else {
        return 0.0;
    };
    let (last_s, last_v) = points[points.len() - 1];
    if s <= first_s {
        return first_v;
    }
    if s >= last_s {
        return last_v;
    }
    let mut lo = 0usize;
    let mut hi = points.len() - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if points[mid].0 <= s {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let tangent = |i: usize| {
        if i == 0 || i == points.len() - 1 {
            return 0.0;
        }
        let before = (points[i].1 - points[i - 1].1) / (points[i].0 - points[i - 1].0);
        let after = (points[i + 1].1 - points[i].1) / (points[i + 1].0 - points[i].0);
        // Level at a peak, a dip or the end of a level stretch; otherwise
        // the harmonic mean of the two slopes. This keeps the curve between
        // its points, so a profile never overshoots the values written.
        if before * after <= 0.0 {
            0.0
        } else {
            2.0 * before * after / (before + after)
        }
    };
    let (s0, v0) = points[lo];
    let (s1, v1) = points[hi];
    let h = s1 - s0;
    let t = (s - s0) / h;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * v0
        + (t3 - 2.0 * t2 + t) * h * tangent(lo)
        + (-2.0 * t3 + 3.0 * t2) * v1
        + (t3 - t2) * h * tangent(hi)
}

/// Terrain built from a [`TerrainDef`].
#[derive(Clone, Debug)]
struct Terrain {
    path: usize,
    corridor: f64,
    blend: f64,
    /// `(distance, height)`.
    elevation: Vec<(f64, f64)>,
    /// `(distance, tangent of the banking angle)`.
    banking: Vec<(f64, f64)>,
    /// Elevation points as world positions `[x, y, height]`.
    anchors: Vec<[f64; 3]>,
}

/// Start position and heading.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SpawnDef {
    /// World x, m.
    pub x: f64,
    /// World y, m.
    pub y: f64,
    /// Heading, degrees counter-clockwise from the x axis.
    pub heading_deg: f64,
}

/// A path made of straights and arcs.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PathDef {
    /// Name used by regions, cone rows and gates.
    pub id: String,
    /// Start x, m.
    pub start_x: f64,
    /// Start y, m.
    pub start_y: f64,
    /// Start heading, degrees.
    pub start_heading_deg: f64,
    /// Segments in order.
    pub segments: Vec<SegmentDef>,
}

/// One path segment.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SegmentDef {
    /// A straight.
    Straight {
        /// Length, m.
        length_m: f64,
    },
    /// A circular arc. Positive angles turn left.
    Arc {
        /// Radius, m.
        radius_m: f64,
        /// Signed turn angle, degrees.
        angle_deg: f64,
    },
}

/// Height profile a rectangular region may carry.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RidgeProfile {
    /// Peak height of the ridges, m.
    pub height_m: f64,
    /// Distance between ridge peaks along the region's length, m.
    pub pitch_m: f64,
}

/// A surface region.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RegionDef {
    /// An oriented rectangle.
    Rect {
        /// Surface identifier.
        surface: String,
        /// Centre x, m.
        center_x: f64,
        /// Centre y, m.
        center_y: f64,
        /// Half extent along the heading, m.
        half_length_m: f64,
        /// Half extent across the heading, m.
        half_width_m: f64,
        /// Heading of the long axis, degrees.
        #[serde(default)]
        heading_deg: f64,
        /// Optional ridge profile along the long axis.
        #[serde(default)]
        ridges: Option<RidgeProfile>,
    },
    /// A disc.
    Circle {
        /// Surface identifier.
        surface: String,
        /// Centre x, m.
        center_x: f64,
        /// Centre y, m.
        center_y: f64,
        /// Radius, m.
        radius_m: f64,
    },
    /// A strip of constant half-width along part of a path.
    Path {
        /// Surface identifier.
        surface: String,
        /// Path name.
        path: String,
        /// Start distance along the path, m.
        from_m: f64,
        /// End distance along the path, m.
        to_m: f64,
        /// Half width, m.
        half_width_m: f64,
    },
    /// A raised kerb beside part of a path, solid for the tires: the ground
    /// rises across its width to `height_m` and falls again at its outer
    /// edge.
    Kerb {
        /// Surface identifier.
        surface: String,
        /// Path name.
        path: String,
        /// Start distance along the path, m.
        from_m: f64,
        /// End distance along the path, m.
        to_m: f64,
        /// Which side of the path, seen in its direction.
        side: KerbSide,
        /// Distance from the path to the inner edge, m.
        inner_m: f64,
        /// Width across, m.
        width_m: f64,
        /// Height of the top above the road, m.
        height_m: f64,
    },
}

/// Side of a path.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KerbSide {
    /// Left of the path direction.
    Left,
    /// Right of the path direction.
    Right,
}

/// Shape of a kerb across its width: height as a fraction of the full
/// height at `t`, 0 at the inner edge to 1 at the outer edge. It ramps up
/// from the road over the first 30 percent, is flat, and drops back to the
/// ground over the last 15 percent.
pub fn kerb_profile(t: f64) -> f64 {
    if !(0.0..=1.0).contains(&t) {
        0.0
    } else if t < KERB_RAMP_IN {
        t / KERB_RAMP_IN
    } else if t > 1.0 - KERB_RAMP_OUT {
        (1.0 - t) / KERB_RAMP_OUT
    } else {
        1.0
    }
}

const KERB_RAMP_IN: f64 = 0.3;
const KERB_RAMP_OUT: f64 = 0.15;

/// One cone.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConeDef {
    /// World x, m.
    pub x: f64,
    /// World y, m.
    pub y: f64,
}

/// Pairs of cones either side of a path.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConePairsDef {
    /// Path name.
    pub path: String,
    /// First pair at this distance along the path, m.
    pub from_m: f64,
    /// No pair beyond this distance, m.
    pub to_m: f64,
    /// Distance between pairs, m.
    pub spacing_m: f64,
    /// Distance of each cone from the path, m.
    pub half_width_m: f64,
}

/// What crossing a gate does.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    /// Starts a timed run.
    Start,
    /// Ends a timed run.
    Finish,
    /// Ends the current lap and starts the next.
    StartFinish,
    /// Must be crossed, in order with the other checkpoints, for a run to
    /// count. Not drawn.
    Checkpoint,
}

/// A timing gate across a path.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GateDef {
    /// What crossing it does.
    pub kind: GateKind,
    /// Path the gate sits on.
    pub path: String,
    /// Distance along the path, m.
    pub at_m: f64,
    /// Half width of the gate, m.
    pub half_width_m: f64,
}

/// A painted marking. Has no effect on the simulation.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MarkerDef {
    /// A painted circle.
    Circle {
        /// Centre x, m.
        center_x: f64,
        /// Centre y, m.
        center_y: f64,
        /// Radius, m.
        radius_m: f64,
        /// Caption.
        #[serde(default)]
        label: String,
    },
    /// A painted line.
    Line {
        /// Start x, m.
        x1: f64,
        /// Start y, m.
        y1: f64,
        /// End x, m.
        x2: f64,
        /// End y, m.
        y2: f64,
        /// Caption.
        #[serde(default)]
        label: String,
    },
}

/// A path expanded into a polyline.
#[derive(Clone, Debug, PartialEq)]
pub struct PathGeom {
    /// Name.
    pub id: String,
    /// Points along the path.
    pub points: Vec<[f64; 2]>,
    /// Distance along the path at each point, m.
    pub s: Vec<f64>,
    /// Heading at each point, rad.
    pub heading: Vec<f64>,
    /// Bounding box `[min_x, min_y, max_x, max_y]` of each chunk of
    /// segments.
    chunks: Vec<[f64; 4]>,
}

impl PathGeom {
    fn build(def: &PathDef) -> Result<Self, String> {
        let mut x = def.start_x;
        let mut y = def.start_y;
        let mut heading = def.start_heading_deg.to_radians();
        let mut dist = 0.0;
        let mut points = vec![[x, y]];
        let mut s = vec![0.0];
        let mut headings = vec![heading];
        for seg in &def.segments {
            match *seg {
                SegmentDef::Straight { length_m } => {
                    if !(length_m.is_finite() && length_m > 0.0) {
                        return Err(format!(
                            "path `{}`: straight length must be positive",
                            def.id
                        ));
                    }
                    let n = (length_m / PATH_STEP_M).ceil().max(1.0) as usize;
                    let step = length_m / n as f64;
                    for _ in 0..n {
                        x += step * fm::cos(heading);
                        y += step * fm::sin(heading);
                        dist += step;
                        points.push([x, y]);
                        s.push(dist);
                        headings.push(heading);
                    }
                }
                SegmentDef::Arc {
                    radius_m,
                    angle_deg,
                } => {
                    if !(radius_m.is_finite() && radius_m > 0.0)
                        || !angle_deg.is_finite()
                        || angle_deg == 0.0
                    {
                        return Err(format!(
                            "path `{}`: arc needs a positive radius and a non-zero angle",
                            def.id
                        ));
                    }
                    let angle = angle_deg.to_radians();
                    let length = radius_m * angle.abs();
                    let n = (length / PATH_STEP_M)
                        .ceil()
                        .max((angle.abs() / PATH_STEP_RAD).ceil())
                        .max(1.0) as usize;
                    let dtheta = angle / n as f64;
                    // Each step is a chord of the arc.
                    let chord = 2.0 * radius_m * fm::sin(0.5 * dtheta.abs());
                    for _ in 0..n {
                        let mid = heading + 0.5 * dtheta;
                        x += chord * fm::cos(mid);
                        y += chord * fm::sin(mid);
                        heading += dtheta;
                        dist += radius_m * dtheta.abs();
                        points.push([x, y]);
                        s.push(dist);
                        headings.push(heading);
                    }
                }
            }
        }
        if points.len() < 2 {
            return Err(format!("path `{}` has no segments", def.id));
        }
        let mut chunks = Vec::new();
        let segments = points.len() - 1;
        let mut first = 0;
        while first < segments {
            let last = (first + CHUNK).min(segments);
            let mut b = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for p in &points[first..=last] {
                b[0] = b[0].min(p[0]);
                b[1] = b[1].min(p[1]);
                b[2] = b[2].max(p[0]);
                b[3] = b[3].max(p[1]);
            }
            chunks.push(b);
            first = last;
        }
        Ok(Self {
            id: def.id.clone(),
            points,
            s,
            heading: headings,
            chunks,
        })
    }

    /// Total length, m.
    pub fn length(&self) -> f64 {
        *self.s.last().unwrap_or(&0.0)
    }

    /// Position and heading at distance `dist` along the path, clamped to
    /// its ends.
    pub fn at(&self, dist: f64) -> ([f64; 2], f64) {
        let d = dist.clamp(0.0, self.length());
        // Binary search for the segment containing `d`.
        let mut lo = 0usize;
        let mut hi = self.s.len() - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self.s[mid] <= d {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let span = self.s[hi] - self.s[lo];
        let t = if span > 0.0 {
            (d - self.s[lo]) / span
        } else {
            0.0
        };
        let a = self.points[lo];
        let b = self.points[hi];
        (
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
            self.heading[lo] + (self.heading[hi] - self.heading[lo]) * t,
        )
    }

    /// Closest point on the part of the path between `from` and `to`:
    /// returns `(distance along the path, signed lateral offset)` with the
    /// offset positive to the left of the path direction, or `None` if
    /// nothing in range lies within `max_offset`.
    pub fn project(
        &self,
        x: f64,
        y: f64,
        from: f64,
        to: f64,
        max_offset: f64,
    ) -> Option<(f64, f64)> {
        let mut best: Option<(f64, f64, f64)> = None; // (dist², s, signed offset)
        let limit2 = max_offset * max_offset;
        for (c, b) in self.chunks.iter().enumerate() {
            if x < b[0] - max_offset
                || x > b[2] + max_offset
                || y < b[1] - max_offset
                || y > b[3] + max_offset
            {
                continue;
            }
            let first = c * CHUNK;
            let last = (first + CHUNK).min(self.points.len() - 1);
            for i in first..last {
                if self.s[i + 1] < from || self.s[i] > to {
                    continue;
                }
                let a = self.points[i];
                let p = self.points[i + 1];
                let (dx, dy) = (p[0] - a[0], p[1] - a[1]);
                let len2 = dx * dx + dy * dy;
                if len2 <= 0.0 {
                    continue;
                }
                let t = (((x - a[0]) * dx + (y - a[1]) * dy) / len2).clamp(0.0, 1.0);
                let along = self.s[i] + t * (self.s[i + 1] - self.s[i]);
                if along < from || along > to {
                    continue;
                }
                let (ox, oy) = (x - (a[0] + t * dx), y - (a[1] + t * dy));
                let d2 = ox * ox + oy * oy;
                if d2 <= limit2 && best.is_none_or(|b| d2 < b.0) {
                    let side = if dx * oy - dy * ox >= 0.0 { 1.0 } else { -1.0 };
                    best = Some((d2, along, side * fm::sqrt(d2)));
                }
            }
        }
        best.map(|(_, along, offset)| (along, offset))
    }
}

#[derive(Clone, Debug)]
enum Region {
    Rect {
        cx: f64,
        cy: f64,
        cos: f64,
        sin: f64,
        half_length: f64,
        half_width: f64,
        surface: u16,
        ridges: Option<RidgeProfile>,
    },
    Circle {
        cx: f64,
        cy: f64,
        radius2: f64,
        surface: u16,
    },
    Path {
        path: usize,
        from: f64,
        to: f64,
        half_width: f64,
        surface: u16,
    },
    Kerb {
        path: usize,
        from: f64,
        to: f64,
        /// +1 for left, -1 for right.
        side: f64,
        inner: f64,
        width: f64,
        height: f64,
        surface: u16,
    },
}

/// A timing gate as a line segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gate {
    /// What crossing it does.
    pub kind: GateKind,
    /// One end.
    pub a: [f64; 2],
    /// The other end.
    pub b: [f64; 2],
    /// Direction of travel through the gate, a unit vector. Crossing the
    /// other way does nothing.
    pub forward: [f64; 2],
    /// Distance along its path, m.
    pub at_m: f64,
}

impl Gate {
    /// Whether moving from `p` to `q` crosses the gate in its direction.
    pub fn crossed_forward(&self, p: [f64; 2], q: [f64; 2]) -> bool {
        segments_cross(p, q, self.a, self.b)
            && (q[0] - p[0]) * self.forward[0] + (q[1] - p[1]) * self.forward[1] > 0.0
    }
}

/// Share of the timed length a run must cover to count.
pub const MIN_RUN_FRACTION: f64 = 0.8;

/// Length of a timed run: start to finish along the start gate's path, or
/// that whole path for a start/finish line.
fn timed_length(gates: &[Gate], paths: &[PathGeom]) -> f64 {
    let find = |kind: GateKind| gates.iter().find(|g| g.kind == kind);
    if find(GateKind::StartFinish).is_some() {
        return paths.first().map_or(0.0, PathGeom::length);
    }
    match (find(GateKind::Start), find(GateKind::Finish)) {
        (Some(s), Some(f)) => (f.at_m - s.at_m).abs(),
        _ => 0.0,
    }
}

/// A track ready for simulation.
#[derive(Clone, Debug)]
pub struct Track {
    def: TrackDef,
    surfaces: Vec<SurfaceParams>,
    default_surface: u16,
    regions: Vec<Region>,
    paths: Vec<PathGeom>,
    cones: Vec<[f64; 2]>,
    gates: Vec<Gate>,
    terrain: Option<Terrain>,
    trees: Vec<Tree>,
    checkpoints: Vec<Gate>,
    timed_length_m: f64,
    walls: Vec<Wall>,
    limits: Option<(usize, f64)>,
    legacy_kerbs: bool,
}

/// Places trees beside the paths: from points along each path, a few tries
/// each at a random distance to either side, thicker close to the road,
/// keeping only those clear of every path.
fn scatter_trees(def: &TreesDef, paths: &[PathGeom]) -> Vec<Tree> {
    let mut rng = crate::drivers::Rng::new(def.seed);
    let mut road: Vec<([f64; 2], f64)> = Vec::new();
    for path in paths {
        let mut d = 0.0;
        while d <= path.length() {
            road.push(path.at(d));
            d += def.spacing_m;
        }
    }
    let clear2 = def.keep_clear_m * def.keep_clear_m;
    let mut trees = Vec::new();
    for &(p, h) in &road {
        for _ in 0..def.per_point {
            let u = rng.unit();
            let reach = def.keep_clear_m + 1.0 + (def.reach_m - def.keep_clear_m - 1.0) * u * u;
            let side = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
            let jitter = [rng.range(-4.0, 4.0), rng.range(-4.0, 4.0)];
            let size = rng.range(0.7, 1.5);
            let pos = [
                p[0] - fm::sin(h) * side * reach + jitter[0],
                p[1] + fm::cos(h) * side * reach + jitter[1],
            ];
            let clear = road.iter().all(|(r, _)| {
                let (dx, dy) = (pos[0] - r[0], pos[1] - r[1]);
                dx * dx + dy * dy >= clear2
            });
            if clear {
                trees.push(Tree {
                    pos,
                    trunk_radius: TRUNK_RADIUS_M * size,
                    size,
                });
            }
        }
    }
    trees
}

fn find_surface(surfaces: &[SurfaceParams], id: &str, track: &str) -> Result<u16, String> {
    surfaces
        .iter()
        .position(|s| s.id == id)
        .map(|i| i as u16)
        .ok_or_else(|| format!("track `{track}` uses unknown surface `{id}`"))
}

impl Track {
    /// Builds a track from its definition and the session's surface table.
    pub fn new(def: TrackDef, surfaces: Vec<SurfaceParams>) -> Result<Self, String> {
        if surfaces.is_empty() || surfaces.len() > usize::from(u16::MAX) {
            return Err("a track needs between 1 and 65535 surfaces".into());
        }
        let default_surface = find_surface(&surfaces, &def.default_surface, &def.id)?;
        let mut paths = Vec::new();
        for p in &def.paths {
            if paths.iter().any(|g: &PathGeom| g.id == p.id) {
                return Err(format!("track `{}` defines path `{}` twice", def.id, p.id));
            }
            paths.push(PathGeom::build(p)?);
        }
        let path_index = |name: &str| {
            paths
                .iter()
                .position(|g| g.id == name)
                .ok_or_else(|| format!("track `{}` refers to unknown path `{name}`", def.id))
        };
        let mut regions = Vec::new();
        for r in &def.regions {
            regions.push(match r {
                RegionDef::Rect {
                    surface,
                    center_x,
                    center_y,
                    half_length_m,
                    half_width_m,
                    heading_deg,
                    ridges,
                } => {
                    if let Some(p) = ridges {
                        if !(p.pitch_m > 0.0 && p.height_m >= 0.0) {
                            return Err(format!(
                                "track `{}`: ridge profile needs a positive pitch",
                                def.id
                            ));
                        }
                    }
                    let h = heading_deg.to_radians();
                    Region::Rect {
                        cx: *center_x,
                        cy: *center_y,
                        cos: fm::cos(h),
                        sin: fm::sin(h),
                        half_length: *half_length_m,
                        half_width: *half_width_m,
                        surface: find_surface(&surfaces, surface, &def.id)?,
                        ridges: *ridges,
                    }
                }
                RegionDef::Circle {
                    surface,
                    center_x,
                    center_y,
                    radius_m,
                } => Region::Circle {
                    cx: *center_x,
                    cy: *center_y,
                    radius2: radius_m * radius_m,
                    surface: find_surface(&surfaces, surface, &def.id)?,
                },
                RegionDef::Path {
                    surface,
                    path,
                    from_m,
                    to_m,
                    half_width_m,
                } => Region::Path {
                    path: path_index(path)?,
                    from: *from_m,
                    to: *to_m,
                    half_width: *half_width_m,
                    surface: find_surface(&surfaces, surface, &def.id)?,
                },
                RegionDef::Kerb {
                    surface,
                    path,
                    from_m,
                    to_m,
                    side,
                    inner_m,
                    width_m,
                    height_m,
                } => {
                    if !(*width_m > 0.0 && *inner_m >= 0.0 && (0.0..=0.2).contains(height_m)) {
                        return Err(format!(
                            "track `{}`: a kerb needs a positive width and a height of at most 0.2 m",
                            def.id
                        ));
                    }
                    Region::Kerb {
                        path: path_index(path)?,
                        from: *from_m,
                        to: *to_m,
                        side: if *side == KerbSide::Left { 1.0 } else { -1.0 },
                        inner: *inner_m,
                        width: *width_m,
                        height: *height_m,
                        surface: find_surface(&surfaces, surface, &def.id)?,
                    }
                }
            });
        }
        let mut cones: Vec<[f64; 2]> = def.cones.iter().map(|c| [c.x, c.y]).collect();
        for row in &def.cone_pairs {
            if row.spacing_m.is_nan() || row.spacing_m <= 0.0 {
                return Err(format!("track `{}`: cone spacing must be positive", def.id));
            }
            let geom = &paths[path_index(&row.path)?];
            let end = row.to_m.min(geom.length());
            let mut d = row.from_m;
            while d <= end + 1e-9 {
                let (p, h) = geom.at(d);
                let (nx, ny) = (-fm::sin(h), fm::cos(h));
                cones.push([p[0] + nx * row.half_width_m, p[1] + ny * row.half_width_m]);
                cones.push([p[0] - nx * row.half_width_m, p[1] - ny * row.half_width_m]);
                d += row.spacing_m;
            }
        }
        let mut gates = Vec::new();
        for g in &def.gates {
            let geom = &paths[path_index(&g.path)?];
            let (p, h) = geom.at(g.at_m);
            let (nx, ny) = (-fm::sin(h), fm::cos(h));
            gates.push(Gate {
                kind: g.kind,
                a: [p[0] + nx * g.half_width_m, p[1] + ny * g.half_width_m],
                b: [p[0] - nx * g.half_width_m, p[1] - ny * g.half_width_m],
                forward: [fm::cos(h), fm::sin(h)],
                at_m: g.at_m,
            });
        }
        // Checkpoints are kept in the order they are met along the path.
        let mut checkpoints: Vec<Gate> = gates
            .iter()
            .filter(|g| g.kind == GateKind::Checkpoint)
            .copied()
            .collect();
        checkpoints.sort_by(|a, b| a.at_m.total_cmp(&b.at_m));
        gates.retain(|g| g.kind != GateKind::Checkpoint);
        let timed_length_m = timed_length(&gates, &paths);
        let terrain = match &def.terrain {
            None => None,
            Some(t) => {
                let path = path_index(&t.path)?;
                let increasing = |d: &[f64]| d.windows(2).all(|w| w[1] > w[0]);
                let elevation_at: Vec<f64> = t.elevation.iter().map(|p| p.at_m).collect();
                let banking_at: Vec<f64> = t.banking.iter().map(|p| p.at_m).collect();
                if t.elevation.is_empty()
                    || !increasing(&elevation_at)
                    || !increasing(&banking_at)
                    || !(t.corridor_half_width_m > 0.0 && t.blend_m > 0.0)
                {
                    return Err(format!(
                        "track `{}`: terrain needs elevation points in increasing order of \
                         distance and a positive corridor and blend width",
                        def.id
                    ));
                }
                Some(Terrain {
                    path,
                    corridor: t.corridor_half_width_m,
                    blend: t.blend_m,
                    elevation: t.elevation.iter().map(|p| (p.at_m, p.height_m)).collect(),
                    banking: t
                        .banking
                        .iter()
                        .map(|p| (p.at_m, fm::tan(p.angle_deg.to_radians())))
                        .collect(),
                    anchors: t
                        .elevation
                        .iter()
                        .map(|p| {
                            let (pos, _) = paths[path].at(p.at_m);
                            [pos[0], pos[1], p.height_m]
                        })
                        .collect(),
                })
            }
        };
        let trees = match &def.trees {
            None => Vec::new(),
            Some(t) => {
                if !(t.spacing_m > 0.0 && t.keep_clear_m > 0.0 && t.reach_m > t.keep_clear_m) {
                    return Err(format!(
                        "track `{}`: trees need a positive spacing and a reach beyond the clear strip",
                        def.id
                    ));
                }
                scatter_trees(t, &paths)
            }
        };
        let mut walls = Vec::new();
        for b in &def.barriers {
            let geom = &paths[path_index(&b.path)?];
            let end = b.to_m.min(geom.length());
            if end.is_nan() || end <= b.from_m {
                return Err(format!("track `{}`: a barrier has no length", def.id));
            }
            let pieces = ((end - b.from_m) / WALL_STEP_M).ceil().max(1.0) as usize;
            let at = |i: usize| {
                let d = b.from_m + (end - b.from_m) * i as f64 / pieces as f64;
                let (p, h) = geom.at(d);
                [
                    p[0] - fm::sin(h) * b.offset_m,
                    p[1] + fm::cos(h) * b.offset_m,
                ]
            };
            for i in 0..pieces {
                walls.push(Wall {
                    a: at(i),
                    b: at(i + 1),
                });
            }
        }
        let limits = match &def.track_limits {
            None => None,
            Some(l) => {
                let index = def
                    .paths
                    .iter()
                    .position(|p| p.id == l.path)
                    .ok_or_else(|| {
                        format!(
                            "track `{}`: track limits name unknown path `{}`",
                            def.id, l.path
                        )
                    })?;
                if !(l.half_width_m.is_finite() && l.half_width_m > 0.0) {
                    return Err(format!(
                        "track `{}`: track limits need a positive half width",
                        def.id
                    ));
                }
                Some((index, l.half_width_m))
            }
        };
        Ok(Self {
            def,
            surfaces,
            default_surface,
            regions,
            paths,
            cones,
            gates,
            terrain,
            checkpoints,
            timed_length_m,
            trees,
            walls,
            limits,
            legacy_kerbs: false,
        })
    }

    /// The definition this track was built from.
    pub fn def(&self) -> &TrackDef {
        &self.def
    }

    /// The session's surface table; a surface index points into it.
    pub fn surfaces(&self) -> &[SurfaceParams] {
        &self.surfaces
    }

    /// All paths.
    pub fn paths(&self) -> &[PathGeom] {
        &self.paths
    }

    /// A path by name.
    pub fn path(&self, id: &str) -> Option<&PathGeom> {
        self.paths.iter().find(|p| p.id == id)
    }

    /// Cone positions.
    pub fn cones(&self) -> &[[f64; 2]] {
        &self.cones
    }

    /// Timing gates.
    pub fn gates(&self) -> &[Gate] {
        &self.gates
    }

    /// Checkpoints, in the order a run must cross them.
    pub fn checkpoints(&self) -> &[Gate] {
        &self.checkpoints
    }

    /// Length of a timed run along the path, m: from the start gate to the
    /// finish gate, or one lap for a start/finish line. Zero without gates.
    pub fn timed_length_m(&self) -> f64 {
        self.timed_length_m
    }

    /// Whether a run whose positions are `points`, from start to finish,
    /// took the course: every checkpoint crossed forward and in order, and
    /// most of the timed length covered. Used to check a saved lap.
    pub fn run_follows_course(&self, points: &[[f64; 2]]) -> bool {
        let mut next = 0;
        let mut covered = 0.0;
        for pair in points.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            covered += fm::hypot(q[0] - p[0], q[1] - p[1]);
            if let Some(c) = self.checkpoints.get(next) {
                if c.crossed_forward(p, q) {
                    next += 1;
                }
            }
        }
        next == self.checkpoints.len() && covered >= MIN_RUN_FRACTION * self.timed_length_m
    }

    /// Solid trees.
    pub fn trees(&self) -> &[Tree] {
        &self.trees
    }

    /// Solid barrier pieces.
    pub fn walls(&self) -> &[Wall] {
        &self.walls
    }

    /// Spawn position and heading in radians.
    pub fn spawn(&self) -> (f64, f64, f64) {
        (
            self.def.spawn.x,
            self.def.spawn.y,
            self.def.spawn.heading_deg.to_radians(),
        )
    }

    /// Height of the shaped ground at a point, m: the road and terrain,
    /// without kerb ridges or surface roughness. Zero on a flat track.
    pub fn ground_height(&self, x: f64, y: f64) -> f64 {
        let Some(t) = &self.terrain else {
            return 0.0;
        };
        // Smooth terrain: the elevation points, weighted by closeness.
        let mut sum = 0.0;
        let mut weight = 0.0;
        for a in &t.anchors {
            let (dx, dy) = (x - a[0], y - a[1]);
            let d2 = dx * dx + dy * dy + TERRAIN_SMOOTHING_M * TERRAIN_SMOOTHING_M;
            let w = 1.0 / (d2 * d2);
            sum += w * a[2];
            weight += w;
        }
        let terrain = sum / weight;
        let path = &self.paths[t.path];
        match path.project(x, y, 0.0, path.length(), t.corridor + t.blend) {
            None => terrain,
            Some((s, offset)) => {
                let across = offset.clamp(-t.corridor, t.corridor);
                let road = profile(&t.elevation, s) + across * profile(&t.banking, s);
                let blend = smoothstep((offset.abs() - t.corridor) / t.blend);
                road + (terrain - road) * blend
            }
        }
    }

    /// Unit normal of the shaped ground at a point.
    pub fn ground_normal(&self, x: f64, y: f64) -> Vec3 {
        if self.terrain.is_none() {
            return Vec3::Z;
        }
        let d = SLOPE_STEP_M;
        let slope_x = (self.ground_height(x + d, y) - self.ground_height(x - d, y)) / (2.0 * d);
        let slope_y = (self.ground_height(x, y + d) - self.ground_height(x, y - d)) / (2.0 * d);
        Vec3::new(-slope_x, -slope_y, 1.0).normalized_or(Vec3::Z)
    }

    /// True if the track has elevation.
    pub fn has_terrain(&self) -> bool {
        self.terrain.is_some()
    }

    /// Surface index and ridge height at a point, ignoring roughness.
    pub fn surface_at(&self, x: f64, y: f64) -> (u16, f64) {
        let (surface, ridge, _) = self.surface_detail(x, y);
        (surface, ridge)
    }

    /// True if the track has limits a lap must stay within.
    pub fn has_limits(&self) -> bool {
        self.limits.is_some()
    }

    /// True if a point is on the track for the purpose of track limits: on
    /// the road or on a kerb. Always true on a track without limits.
    pub fn within_limits(&self, x: f64, y: f64) -> bool {
        let Some((path, half)) = self.limits else {
            return true;
        };
        let geom = &self.paths[path];
        if geom.project(x, y, 0.0, geom.length(), half).is_some() {
            return true;
        }
        self.regions.iter().any(|r| match r {
            Region::Kerb {
                path,
                from,
                to,
                side,
                inner,
                width,
                ..
            } => self.paths[*path]
                .project(x, y, *from, *to, inner + width)
                .is_some_and(|(_, offset)| (0.0..=*width).contains(&(offset * side - inner))),
            _ => false,
        })
    }

    /// Puts the kerbs back as they were before they pushed sideways: a
    /// level contact normal and square ends. Only for comparing the two
    /// models; sessions never use it.
    #[doc(hidden)]
    pub fn set_legacy_kerbs(&mut self, legacy: bool) {
        self.legacy_kerbs = legacy;
    }

    /// Surface index, ridge height and the slope of the kerb under a point
    /// (height gradient in world x and y, averaged over a tire's width).
    fn surface_detail(&self, x: f64, y: f64) -> (u16, f64, [f64; 2]) {
        let mut surface = self.default_surface;
        let mut ridge = 0.0;
        let mut slope = [0.0; 2];
        for region in &self.regions {
            match region {
                Region::Rect {
                    cx,
                    cy,
                    cos,
                    sin,
                    half_length,
                    half_width,
                    surface: s,
                    ridges,
                } => {
                    let (dx, dy) = (x - cx, y - cy);
                    let u = dx * cos + dy * sin;
                    let v = -dx * sin + dy * cos;
                    if u.abs() <= *half_length && v.abs() <= *half_width {
                        surface = *s;
                        ridge = match ridges {
                            Some(p) => {
                                let phase = (u / p.pitch_m).rem_euclid(1.0);
                                p.height_m * (1.0 - (2.0 * phase - 1.0).abs())
                            }
                            None => 0.0,
                        };
                    }
                }
                Region::Circle {
                    cx,
                    cy,
                    radius2,
                    surface: s,
                } => {
                    let (dx, dy) = (x - cx, y - cy);
                    if dx * dx + dy * dy <= *radius2 {
                        surface = *s;
                        ridge = 0.0;
                    }
                }
                Region::Path {
                    path,
                    from,
                    to,
                    half_width,
                    surface: s,
                } => {
                    if self.paths[*path]
                        .project(x, y, *from, *to, *half_width)
                        .is_some()
                    {
                        surface = *s;
                        ridge = 0.0;
                    }
                }
                Region::Kerb {
                    path,
                    from,
                    to,
                    side,
                    inner,
                    width,
                    height,
                    surface: s,
                } => {
                    let half = 0.5 * KERB_ENVELOPE_M;
                    if let Some((along, offset)) =
                        self.paths[*path].project(x, y, *from, *to, inner + width + half)
                    {
                        let across = offset * side - inner;
                        let legacy = self.legacy_kerbs;
                        // Height across the kerb, tapered at both ends.
                        let h = |a: f64, s_along: f64| {
                            let taper = if legacy {
                                1.0
                            } else {
                                ((s_along - from) / KERB_END_TAPER_M)
                                    .min((to - s_along) / KERB_END_TAPER_M)
                                    .clamp(0.0, 1.0)
                            };
                            height * kerb_profile(a / width) * taper
                        };
                        if (0.0..=*width).contains(&across) {
                            surface = *s;
                            ridge = h(across, along);
                        }
                        // The slope reaches half a tire beyond the edges,
                        // where the tread already rests partly on the ramp.
                        if !legacy && (-half..=*width + half).contains(&across) {
                            let rise_across = (h(across + half, along) - h(across - half, along))
                                / KERB_ENVELOPE_M;
                            let rise_along = (h(across, along + half) - h(across, along - half))
                                / KERB_ENVELOPE_M;
                            let (_, heading) = self.paths[*path].at(along);
                            // Across increases away from the path: toward
                            // its left on a left kerb, its right on a right.
                            let (sin, cos) = (fm::sin(heading), fm::cos(heading));
                            slope = [
                                -sin * side * rise_across + cos * rise_along,
                                cos * side * rise_across + sin * rise_along,
                            ];
                        }
                    }
                }
            }
        }
        (surface, ridge, slope)
    }
}

impl Ground for Track {
    fn sample(&self, x: f64, y: f64) -> GroundSample<'_> {
        let (index, ridge, slope) = self.surface_detail(x, y);
        let surface = &self.surfaces[usize::from(index)];
        let rough = if surface.roughness_amplitude_m > 0.0 {
            let k = 1.0 / surface.roughness_wavelength_m;
            surface.roughness_amplitude_m * lattice_noise_2d(x * k, y * k)
        } else {
            0.0
        };
        let mut normal = self.ground_normal(x, y);
        if slope != [0.0, 0.0] {
            // Add the kerb's slope to the terrain's and renormalise.
            let nz = normal.z.max(1e-6);
            let gx = -normal.x / nz + slope[0];
            let gy = -normal.y / nz + slope[1];
            normal = Vec3::new(-gx, -gy, 1.0).normalized_or(Vec3::Z);
        }
        GroundSample {
            height: self.ground_height(x, y) + ridge + rough,
            normal,
            surface,
            surface_index: index,
        }
    }
}

/// True if segment `p0→p1` crosses segment `a→b`.
pub fn segments_cross(p0: [f64; 2], p1: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }
    let d1 = orient(a, b, p0);
    let d2 = orient(a, b, p1);
    let d3 = orient(p0, p1, a);
    let d4 = orient(p0, p1, b);
    // Strict on the travelling segment's start so a point resting exactly on
    // the gate line does not trigger every tick.
    ((d1 < 0.0 && d2 >= 0.0) || (d1 > 0.0 && d2 <= 0.0))
        && ((d3 <= 0.0 && d4 >= 0.0) || (d3 >= 0.0 && d4 <= 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use qts_tire::test_support::{gravel, tarmac};

    fn def(text: &str) -> TrackDef {
        toml::from_str(text).unwrap()
    }

    const BASE: &str = r#"
        id = "t"
        name = "Test"
        default_surface = "tarmac"
        air_temp_c = 20.0
        road_temp_c = 25.0
        ground_half_extent_m = 100.0
        [spawn]
        x = 0.0
        y = 0.0
        heading_deg = 90.0
    "#;

    fn surfaces() -> Vec<SurfaceParams> {
        vec![gravel(), tarmac()]
    }

    #[test]
    fn path_follows_straights_and_arcs() {
        let d = def(&format!(
            r#"{BASE}
            [[paths]]
            id = "p"
            start_x = 0.0
            start_y = 0.0
            start_heading_deg = 0.0
            segments = [
                {{ kind = "straight", length_m = 10.0 }},
                {{ kind = "arc", radius_m = 20.0, angle_deg = 90.0 }},
                {{ kind = "arc", radius_m = 10.0, angle_deg = -90.0 }},
            ]
            "#
        ));
        let track = Track::new(d, surfaces()).unwrap();
        let p = track.path("p").unwrap();
        let expected_len =
            10.0 + 20.0 * std::f64::consts::FRAC_PI_2 + 10.0 * std::f64::consts::FRAC_PI_2;
        assert!((p.length() - expected_len).abs() < 1e-9);
        // After the left arc: (10 + 20, 20) heading north. After the right
        // arc of radius 10: (40, 30) heading east.
        let (end, heading) = p.at(p.length());
        assert!(
            (end[0] - 40.0).abs() < 1e-6 && (end[1] - 30.0).abs() < 1e-6,
            "{end:?}"
        );
        assert!(heading.abs() < 1e-9);
        let (mid, h) = p.at(5.0);
        assert!((mid[0] - 5.0).abs() < 1e-9 && mid[1].abs() < 1e-9 && h == 0.0);
    }

    #[test]
    fn projection_gives_distance_and_signed_offset() {
        let d = def(&format!(
            r#"{BASE}
            [[paths]]
            id = "p"
            start_x = 0.0
            start_y = 0.0
            start_heading_deg = 0.0
            segments = [{{ kind = "straight", length_m = 100.0 }}]
            "#
        ));
        let track = Track::new(d, surfaces()).unwrap();
        let p = track.path("p").unwrap();
        let (s, off) = p.project(42.3, 1.5, 0.0, 100.0, 5.0).unwrap();
        assert!((s - 42.3).abs() < 1e-9 && (off - 1.5).abs() < 1e-9);
        let (_, off) = p.project(42.3, -2.0, 0.0, 100.0, 5.0).unwrap();
        assert!((off + 2.0).abs() < 1e-9);
        assert!(p.project(42.3, 6.0, 0.0, 100.0, 5.0).is_none());
        assert!(p.project(42.3, 1.0, 50.0, 100.0, 5.0).is_none());
    }

    #[test]
    fn later_regions_override_earlier_ones_and_the_default() {
        let d = def(&format!(
            r#"{BASE}
            [[paths]]
            id = "road"
            start_x = 0.0
            start_y = 50.0
            start_heading_deg = 0.0
            segments = [{{ kind = "straight", length_m = 100.0 }}]
            [[regions]]
            kind = "rect"
            surface = "gravel"
            center_x = 0.0
            center_y = 0.0
            half_length_m = 10.0
            half_width_m = 5.0
            heading_deg = 90.0
            [[regions]]
            kind = "circle"
            surface = "tarmac"
            center_x = 0.0
            center_y = 8.0
            radius_m = 3.0
            [[regions]]
            kind = "path"
            surface = "gravel"
            path = "road"
            from_m = 20.0
            to_m = 60.0
            half_width_m = 3.0
            "#
        ));
        let track = Track::new(d, surfaces()).unwrap();
        let name = |x, y| track.sample(x, y).surface.id.clone();
        assert_eq!(name(50.0, -50.0), "tarmac"); // default
        assert_eq!(name(4.0, 0.0), "gravel"); // inside the rotated rect
        assert_eq!(name(6.0, 0.0), "tarmac"); // outside its width
        assert_eq!(name(0.0, 9.0), "tarmac"); // circle painted over the rect
        assert_eq!(name(0.0, 4.0), "gravel"); // rect outside the circle
        assert_eq!(name(40.0, 52.0), "gravel"); // on the path strip
        assert_eq!(name(40.0, 54.0), "tarmac"); // beside it
        assert_eq!(name(10.0, 50.0), "tarmac"); // before the strip starts
    }

    #[test]
    fn ridges_raise_the_ground_and_roughness_is_repeatable() {
        let d = def(&format!(
            r#"{BASE}
            [[regions]]
            kind = "rect"
            surface = "tarmac"
            center_x = 0.0
            center_y = 0.0
            half_length_m = 10.0
            half_width_m = 1.0
            ridges = {{ height_m = 0.03, pitch_m = 0.5 }}
            "#
        ));
        let track = Track::new(d, surfaces()).unwrap();
        assert!((track.sample(0.25, 0.0).height - 0.03).abs() < 1e-12);
        assert!(track.sample(0.5, 0.0).height.abs() < 1e-12);
        assert_eq!(track.sample(0.25, 5.0).height, 0.0);
        // Gravel is rough; the same point always returns the same height.
        let mut s = surfaces();
        s[1].roughness_amplitude_m = 0.004;
        let d2 = def(BASE);
        let rough = Track::new(d2, s).unwrap();
        let h1 = rough.sample(3.3, -7.1).height;
        assert_eq!(h1, rough.sample(3.3, -7.1).height);
        assert!(h1.abs() <= 0.004);
    }

    #[test]
    fn cone_pairs_and_gates_are_placed_across_the_path() {
        let d = def(&format!(
            r#"{BASE}
            [[paths]]
            id = "p"
            start_x = 0.0
            start_y = 0.0
            start_heading_deg = 0.0
            segments = [{{ kind = "straight", length_m = 30.0 }}]
            [[cone_pairs]]
            path = "p"
            from_m = 0.0
            to_m = 30.0
            spacing_m = 10.0
            half_width_m = 2.0
            [[cones]]
            x = 5.0
            y = 0.0
            [[gates]]
            kind = "start"
            path = "p"
            at_m = 15.0
            half_width_m = 3.0
            "#
        ));
        let track = Track::new(d, surfaces()).unwrap();
        assert_eq!(track.cones().len(), 1 + 2 * 4);
        assert!(track.cones().contains(&[10.0, 2.0]));
        assert!(track.cones().contains(&[10.0, -2.0]));
        let g = track.gates()[0];
        assert!((g.a[0] - 15.0).abs() < 1e-9 && (g.a[1] - 3.0).abs() < 1e-9);
        assert!((g.b[1] + 3.0).abs() < 1e-9);
        assert!(segments_cross([14.9, 0.5], [15.1, 0.5], g.a, g.b));
        assert!(!segments_cross([14.9, 4.0], [15.1, 4.0], g.a, g.b));
        assert!(!segments_cross([14.0, 0.5], [14.9, 0.5], g.a, g.b));
        // Resting on the line does not count as a crossing.
        assert!(!segments_cross([15.0, 0.5], [15.0, 0.5], g.a, g.b));
    }

    #[test]
    fn unknown_names_are_reported() {
        let d = def(&BASE.replace("\"tarmac\"", "\"sand\""));
        assert!(Track::new(d, surfaces()).unwrap_err().contains("sand"));
        let d = def(&format!(
            r#"{BASE}
            [[gates]]
            kind = "finish"
            path = "nowhere"
            at_m = 1.0
            half_width_m = 1.0
            "#
        ));
        assert!(Track::new(d, surfaces()).unwrap_err().contains("nowhere"));
    }
}
