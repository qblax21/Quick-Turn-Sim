<!-- SPDX-License-Identifier: Apache-2.0 -->
# Decision log

Every non-obvious choice, with the reason. Newest entries at the bottom.
"Simplest option" entries are cases where the brief was ambiguous.

## D-001 `qts-core` is the bottom layer

The brief lists the loop, state, snapshot, replay and hashing under
`qts-core`. If `qts-core` also owned the concrete world state it would have to
depend on the vehicle and tire crates, and those could not then use its math
and snapshot traits. `qts-core` therefore owns the mechanism (traits, driver,
replay container) and the concrete session lives in `qts-sim`.

## D-002 `qts-sim` is a library plus a binary

The session, track query and data loader are needed by the headless binary,
the validation harness and the Godot binding. A library target in `qts-sim`
gives them one home without adding a ninth crate. The binary contains no
logic.

## D-003 Hand-written math, no linear-algebra dependency

The sim needs a 3-vector, a quaternion and a 3×3 matrix in `f64`. That is a
few hundred lines, and writing it removes a dependency and keeps every
floating-point operation in the determinism path visible and under our
control.

## D-004 Snapshot bytes are hand-written, and the hash is taken over them

A serialization crate in the snapshot path would make the byte layout depend
on that crate's version. A small `StateWriter` keeps the layout explicit.
Hashing the snapshot bytes (64-bit FNV-1a) means a field cannot be restored
but left out of the hash or the reverse. FNV-1a is not cryptographic; it
detects desync, it does not resist tampering. Anti-cheat is out of scope.

## D-005 Input frames are fixed-point integers

Device values are quantised to `i16` when a frame is built. The sim sees only
the integers, so a replay file or a future network packet carries exactly
what was simulated, with no float-formatting ambiguity.

## D-006 ISO 8855 axes and SI units

x forward, y left, z up. Chosen over SAE (z down) because z-up matches the
world frame and makes the Godot conversion a single axis swap in one file.

## D-007 Unsprung masses are real degrees of freedom

The alternative is a kinematic "ray-cast" suspension where tire load is
computed algebraically. Giving each unsprung mass a vertical degree of freedom
on a tire spring costs four extra states and produces load variation, wheel
hop and road detail from the dynamics, which is what the force-feedback
detail channels need. It is stable at 1000 Hz with semi-implicit Euler.

## D-008 Friction couplings use a synchronising-torque solve

Clutch lock-up, brake lock and limited-slip locking are stiff if modelled as
steep viscous friction. Each is instead solved per tick as the torque that
would bring the two sides to the same speed in this tick, clamped to the
element's capacity. This is unconditionally stable and has no tuning
constant.

## D-009 Reduced Magic Formula subset

A full MF 5.2 implementation has over a hundred coefficients, none of which we
have data for. The baseline implements the terms that the placeholder data
can populate (load sensitivity, camber, combined slip weighting, pneumatic
trail) and keeps PAC2002 coefficient names so the set can grow without
renaming.

## D-010 Transcendental math goes through one module

`qts_core::math::fm` forwards to `std` today. Cross-platform determinism later
needs a software math library; having one switch point makes that a
one-file change. It is not switched now because the claim could not be
tested without a second platform in CI (simplest option).

## D-011 Force feedback is outside the deterministic state

The pipeline's damping, friction and inertia terms depend on the physical
wheel's velocity, which is real-world input at wall-clock time. Including it
in the state hash would make replays diverge. The sim outputs raw torque
deterministically; the pipeline consumes it.

## D-012 The real-time host runs the sim on its own thread

Stepping physics from the render loop would tie force-feedback update rate
and latency to the frame rate. A host thread steps the session at the physics
rate and publishes poses for interpolation. The wall clock decides only how
many ticks to run.

## D-013 TOML data files with per-parameter provenance

Dependencies: `serde` (derive) and `toml`. Both are the de-facto standard,
pure Rust, and used only at load time, never inside a step. Each parameter is
`{ value, source, confidence }`; units are in the key name to keep the table
short.

## D-014 File formats do not contain the product name

Replay magic bytes and telemetry format identifiers are neutral strings. The
human-readable generator line in a telemetry header is filled from
`qts_core::constants`. The Godot window title is set at run time from the
same constant, and `project.godot` uses a neutral project name.

## D-015 Ground is flat in the foundation

The track query returns height, normal and surface, so elevation is a data
and query change later. Kerbs are a height profile on a surface region
(simplest option).

## D-016 ABS is modelled; traction and stability control are not

Both car classes have ABS in reality, and a braking-distance metric without
it would mostly measure the test driver. ABS is a simple per-wheel slip
threshold and can be switched off in the car file. Traction and stability
control are left out; the launch test uses a driver model instead.

## D-017 Suspension geometry is an inclined wheel path, not added forces

The first implementation added jacking and anti-dive as extra forces
proportional to tire force. The undamped energy test showed that this is
not energy-consistent: a force acts on the suspension travel but the wheel
does not move along the matching path. Each wheel now travels along a
direction tilted sideways by `roll_center_height / half_track` and fore-aft
by `anti_pitch_tan`. Jacking, anti-dive, anti-lift and anti-squat fall out
of the equations of motion, and track change with travel comes for free.
One anti-pitch parameter per axle replaces the separate brake and drive
values, so inboard-drive and outboard-brake geometry are not distinguished.

## D-018 Tire forces act on the body at the contact patch

Applying the longitudinal force at the contact patch gives the correct
pitch moment in steady state. It differs from the exact treatment (force
at the wheel centre plus brake and drive reaction torques) by the wheel's
spin-up inertia torque, which is small.

## D-019 Rolling radius equals loaded radius

A separate effective rolling radius would let the tire do net work at zero
slip, which shows up as a small energy source. One radius is used for slip
and for wheel torque (simplest option).

## D-020 Engine stall and restart are implicit

There is no starter model. If the engine is dragged to zero speed it stays
there while coupled; pressing the clutch or selecting neutral lets the idle
controller spin it back up (simplest option).

## D-021 Gearbox efficiency is folded into engine friction

A per-gear efficiency would make the clutch constraint asymmetric. Losses
are represented by the engine friction terms only, so the driveline is
slightly too efficient in high gears.

## D-022 Regeneration stands down while the anti-lock system works

Any wheel in anti-lock modulation switches regeneration off on both axles,
including lift-off regeneration, so the motors cannot fight the modulation.
Friction brakes take the full demand in that case.

## D-023 Open differential shares rotor inertia through the mesh

Each motor is a separate rotating body tied to its axle by a rigid
constraint in the wheel-spin solver, rather than an analytically reduced
inertia. The same solver then handles brakes, rolling resistance, clutch,
limited slip and gear meshes with one mechanism.

## D-024 Car variants inherit from a base file

`inherits = "<id>"` in a car file deep-merges the file over its base. The
limited-slip variant of Car A is ten lines instead of a second copy of
every parameter that could drift apart.

## D-025 Carcass deflection is excluded from the energy function

The tire deflection state stores elastic energy only while the tire is not
sliding. Counting it produced false energy gains on a frictionless
surface, so the energy checks leave it out.
