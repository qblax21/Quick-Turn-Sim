<!-- SPDX-License-Identifier: Apache-2.0 -->
# Data files

Everything the simulation knows about cars, tires, surfaces and tracks is
read from TOML files under `data/`. Nothing is compiled in. The file name
without `.toml` is the identifier, and each file repeats it as `id`.

| Folder | One file per | Numbers need provenance |
| --- | --- | --- |
| `data/cars` | Car | Yes |
| `data/tires` | Tire | Yes |
| `data/surfaces` | Road surface | Yes |
| `data/ffb` | Force-feedback preset | Yes |
| `data/tracks` | Track | No (original layout, not a physical parameter) |
| `data/validation` | Target set for one car | Each target has its own source and confidence |
| `data/import` | Logger column-mapping profile | No |

Units are SI and the unit is the suffix of the key: `mass_kg`,
`spring_rate_n_per_m`, `static_camber_rad`.

## Parameters with provenance

In car, tire, surface and force-feedback files every number is a table:

```toml
sprung_mass_kg = { value = 1229.0, source = "class-typical kerb weight less unsprung mass", confidence = "placeholder" }
ratios = { value = [3.31, 1.92, 1.36], source = "invented", confidence = "placeholder" }
```

- `value`: a number or a list of numbers.
- `source`: where the value came from, in words. Required and non-empty.
- `confidence`: `placeholder` (a stand-in), `estimated` (derived from public
  class-typical figures or simple calculation) or `measured` (from a
  measurement the project holds).
- `note`: optional free text.

A bare number in these files is a load error, as is a missing `source` or
`confidence`. Text and true/false values (names, `kind = "open"`,
`enabled = true`) are written plainly. List what is still a guess with:

```
qts-sim provenance --car hatch_tern_gt
```

## Cars

Sections: `[body]`, `[suspension.front]`, `[suspension.rear]`, `[steering]`,
`[brakes]`, `[brakes.abs]`, `[aero]`, `[drivetrain]` and its sub-tables. The
field list and the meaning of each field are the doc comments in
`crates/qts-vehicle/src/params.rs`; unknown keys are rejected, so a typo is
an error rather than a silently ignored value.

`[drivetrain] kind` selects the module:

- `engine_manual`: `[drivetrain.engine]`, `.clutch`, `.gearbox`,
  `.differential` (`kind = "open"` or `"limited_slip"`), and `driven_axle`.
- `dual_motor_electric`: `[drivetrain.front_motor]`, `.rear_motor`,
  `.battery`, `.regen`, plus `front_torque_share` and `torque_vectoring`.

### Variants

A car file may start with `inherits = "<other car id>"`. It is then merged
over that file: tables merge key by key, and anything it sets replaces the
base value. `data/cars/hatch_tern_gt_lsd.toml` is the example.

## Tires

Sections: `[geometry]`, `[vertical]`, `[magic_formula]`, `[transient]`,
`[thermal]`, `[pressure]`. Magic Formula keys are PAC2002 coefficient names
in lower case. Only the coefficients listed in
`crates/qts-tire/src/params.rs` exist; a published tire file with more
coefficients cannot be loaded directly yet.

## Surfaces

One flat table: friction and stiffness scales, how much of the tire's
post-peak force drop remains (`falloff`), added rolling resistance, a
ploughing coefficient for loose surfaces, and roughness. `tuned = false`
marks a first-pass surface; the front end and reports show that.

## Tracks

```toml
id = "example"
name = "Example"
default_surface = "tarmac"     # surface where no region applies
air_temp_c = 20.0
road_temp_c = 28.0
ground_half_extent_m = 200.0   # how much ground to draw

[spawn]
x = 0.0
y = 0.0
heading_deg = 0.0              # counter-clockwise from +x

[[paths]]                      # straights and arcs; positive angles turn left
id = "course"
start_x = 0.0
start_y = 0.0
start_heading_deg = 0.0
segments = [
    { kind = "straight", length_m = 50.0 },
    { kind = "arc", radius_m = 20.0, angle_deg = 90.0 },
]

[[regions]]                    # later regions win where they overlap
kind = "path"                  # a strip along part of a path
surface = "gravel"
path = "course"
from_m = 0.0
to_m = 60.0
half_width_m = 3.0

[[regions]]
kind = "rect"                  # also: kind = "circle" with radius_m
surface = "kerb"
center_x = 10.0
center_y = -4.0
half_length_m = 5.0
half_width_m = 0.5
heading_deg = 0.0
ridges = { height_m = 0.025, pitch_m = 0.35 }   # optional height profile

[[cone_pairs]]                 # cones either side of a path
path = "course"
from_m = 0.0
to_m = 80.0
spacing_m = 6.0
half_width_m = 2.4

[[cones]]                      # single cones
x = 5.0
y = 1.0

[[gates]]                      # kind: start, finish or start_finish
kind = "start"
path = "course"
at_m = 2.0
half_width_m = 2.4

[[markers]]                    # paint only: kind = "line" or "circle"
kind = "circle"
center_x = 0.0
center_y = 60.0
radius_m = 30.0
label = "skidpad"
```

A track may add elevation and banking along one of its paths:

```toml
[terrain]
path = "circuit"
corridor_half_width_m = 9.0    # the road surface is shaped exactly this far out
blend_m = 16.0                 # then blends into smooth terrain over this width
elevation = [                  # height of the centre line, in order of distance
    { at_m = 0.0, height_m = 0.0 },
    { at_m = 450.0, height_m = -2.0 },
]
banking = [                    # optional; positive raises the left edge
    { at_m = 600.0, angle_deg = -5.0 },
]
```

Heights and banking are joined by a smooth curve that never goes outside the
values written, and is level at the first and last point (so a closed lap
should start and end level, at the same height). Away from the road the
ground is interpolated from the elevation points. Parts of the path that are
not neighbours must stay further apart than twice
`corridor_half_width_m + blend_m`, or the ground between them has a step.

The world is x forward (east on a map), y left (north), in metres. Without
a `terrain` section the ground is flat apart from roughness and ridge
profiles.
`qts-sim track-info <id>` prints path lengths, bounds, how close a path
comes to itself, and the surface along it.

## Validation targets

```toml
car = "hatch_tern_gt"
track = "test_track"           # needs open tarmac and a circle marker
tire_temp_c = 60.0
note = "..."

[[target]]
metric = "skidpad_lateral_g"   # see below
value = 0.93
unit = "g"
tolerance_percent = 10.0
source = "..."
confidence = "placeholder"
```

| `metric` | `unit` |
| --- | --- |
| `skidpad_lateral_g` | `g` |
| `braking_distance_60_0` | `m` |
| `accel_0_60` | `s` |
| `top_speed` | `m/s` |
| `understeer_gradient` | `deg/g` |

How each is measured is defined in `DECISIONS.md` (D-035).

## Force-feedback presets and import profiles

Preset fields are documented in `crates/qts-ffb/src/pipeline.rs`
(`FfbConfig`). Import profiles are described in `docs/TELEMETRY_IMPORT.md`.
