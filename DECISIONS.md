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

## D-026 Dependencies, and why each one is there

- `serde` (derive) and `toml`: parsing data files at load time. Never used
  inside a simulation step.
- `windows` 0.58, Windows only, `qts-ffb` only: typed bindings for the
  DirectInput COM interfaces. Hand-written vtables were the alternative and
  were judged riskier than a generated binding. Version 0.58 is pinned on
  purpose: later versions link through `raw-dylib`, which on the GNU Windows
  toolchain needs a working `dlltool` that a plain rustup install does not
  provide. 0.58 ships prebuilt import libraries and builds on both the MSVC
  and GNU toolchains with nothing extra installed.
- `godot` (gdext), `qts-godot` only: the GDExtension binding.

Nothing else is depended on directly. There is no linear-algebra, logging,
command-line, error-handling or serialization crate in the simulation path.

## D-027 Development toolchain on Windows is GNU; MSVC also supported

The development machine had Visual Studio without a Windows SDK, so the
MSVC Rust target could not link. The `x86_64-pc-windows-gnu` toolchain
carries its own linker and needs nothing else. The code does not depend on
the choice; CI builds with MSVC on Windows and on Linux.

## D-028 Frame-rate input is ramped, not stepped

The front end samples input once per rendered frame, but physics runs at
1000 Hz. Feeding each new value as a step would hit the tires with a
staircase and show up as grain in the force feedback. The real-time host
spreads each change over the interval since the previous input update
(capped at 50 ms). This adds up to one frame of latency to analog inputs
when read through the front end.

## D-029 Optional direct steering from the wheel at the physics rate

When a force-feedback device is open, the host can take the steering angle
straight from it every device update, mapping wheel rotation one-to-one onto
the car's steering wheel. This avoids the frame of latency in D-028 for the
input that matters most. It is off by default because it has not been tried
on hardware.

## D-030 Opening a device briefly stalls the sim thread

DirectInput device enumeration takes a few hundred milliseconds. The device
is opened on the sim thread because that thread is the only one that uses
it, so the simulation pauses for that long when a device is selected in the
menu. The fixed-step driver bounds the catch-up afterwards.

## D-031 Timing gates trigger in either direction

A gate fires when the car's centre crosses its line, whichever way it is
travelling (simplest option). Driving backwards through the finish counts.

## D-032 Cones are detected, not simulated

A cone is knocked down when the car's body outline overlaps it. It has no
mass and does not move the car. The count is part of the deterministic
state.

## D-033 Steering compliance is modelled

The first validation run gave understeer gradients near zero. Without
compliance the only sources of understeer are weight distribution, tire
load sensitivity and camber, and in real cars the twist of the steering
system under aligning moment is a main contributor. One parameter per car
(`steering.compliance_rad_per_nm`) takes steer angle off the front wheels
in proportion to the kingpin moment, through a 20 ms lag so the result of a
tick does not feed back into the same tick. The understeer gradient is
measured from steering-wheel angle over steering ratio, as the standard
definition requires, so compliance counts.

## D-034 Anti-lock control is proportional to slip error

The first controller released and reapplied pressure at fixed rates around
a threshold and lost about 15 percent of the available deceleration. It now
moves pressure at a rate proportional to the difference between target and
actual slip, within the same rate limits. This is still a simple
controller, not a model of any production system.

## D-035 How the validation manoeuvres are defined

- Skidpad: 30 m radius, counter-clockwise, target speed ramped 0.12 m/s per
  second, result is the best one-second mean lateral acceleration while the
  car is within 1.5 m of the line.
- Braking: full pedal applied in one step as the car coasts through 60 mph;
  distance from that point to rest. The build-up of deceleration is
  included.
- 0 to 60 mph: timed from the start of clutch release (or pedal application
  for the electric car), with no rollout allowance. The engine car takes the
  best of nine launches, as a driver would try several.
- Top speed: full throttle from rest on level ground in still air until ten
  seconds add less than 0.05 m/s.
- Understeer gradient: least-squares slope between 0.15 g and 0.45 g from
  the same circle run.

These are documented choices, not the only valid ones. Published figures
for real cars often use a one-foot rollout and may differ by several
percent for that reason alone.

## D-036 What "passing validation" means today

Car parameters were chosen as class-typical placeholders, and two model
gaps found by the first validation run were closed (D-033, D-034). No
parameter was fitted to make a target pass, and the targets themselves are
placeholders. A passing report therefore shows that the harness works and
that the placeholder cars are in a plausible range. It is not evidence of
realism, and the report says so on every run.

## D-037 The binding builds against the Godot 4.4 interface

The `godot` crate is built with `api-4-4`, so one library loads in Godot 4.4
and every later 4.x editor. It was developed and smoke-tested against
Godot 4.7.2. Building against the newest interface would have locked out
older editors for no benefit, since the binding uses only basic types.

## D-038 The binding exposes one class and returns plain values

`SimBridge` returns transforms, packed arrays and dictionaries. There are
no Godot nodes, signals or resources defined in Rust, so the front end can
be rewritten freely and the binding stays a type-conversion layer. Track
geometry is sent as data (points, regions, cones) and meshed in GDScript.

## D-039 The Godot project name is neutral

`project.godot` calls the project `sim-frontend`. The window title and
menu heading come from the simulation core's constants at run time, so the
product name is still written only there and in the README.

## D-040 Front-end verification is a headless smoke test plus screenshots

`godot/tests/smoke.gd` loads every car on every track through the real
binding, builds the menu, setup screen, world, car and HUD, and checks that
physics advances and the drawn car moves in the right direction. A
screenshot tool renders the real main scene for visual inspection. Neither
can verify how a wheel feels or whether DirectInput output works on a given
device; that needs a person at the wheel.

## D-041 The DirectInput backend creates its own hidden window when needed

The first test on real hardware failed at `Acquire` with "invalid window
handle". DirectInput grants exclusive access only through a top-level
window that belongs to the calling process, so borrowing the foreground or
desktop window does not work. When the caller supplies no handle (the
command-line tool), the backend creates a hidden window and destroys it
with the device. The front end passes its own window handle.

## D-042 Power assist is a fixed fraction of the steering load

After the first drive on a wheel, the electric sedan's steering felt a
little heavy while the hatch felt right. `steering.assist_fraction` removes
a fixed share of the kingpin torque before it reaches the column (0 for the
hatch, 0.25 for the sedan). Real assist varies with speed and torque; this
is the simplest model that lets weight be set per car instead of per wheel
preset. The value is driver feedback, not a measurement.

## D-043 First changes made from driving feedback

One driver, one session, on a Fanatec CSL Elite: the hatch had slightly too
little lift-off oversteer, so its rear anti-roll bar went from 16 to
20 kN/m and rear toe-in was halved. The sources in the car file say so.
These are still placeholders; a subjective impression is not a measurement.
The driver's-seat camera became the default view.

## D-044 Autocross course is marked with gates, not a lane of cones

Driver feedback: the first course had too many cones, one width and little
variety. It was a continuous lane of cone pairs every 6 m. The new course
(412 m, 69 cones) is marked the way a club course is: gates of two cones
where the line needs defining, single cones for a five-cone slalom, and a
width that changes with the element (2.7 m half-width at the offset, 5 m in
the sweeper). Elements: slalom, wide sweeper, offset, hairpin, tightening
right, fast straight, medium right. The path in the file is the weaving
driving line, so the scripted driver still runs it clean. No format change:
the existing `cones` and several `cone_pairs` entries were enough. It is an
original layout, not a copy of any real event course.

## D-045 Placeholder sound is synthesised in the front end

The brief had no sound. The driver asked for engine and tire sound to
judge the cars by, so the front end now makes it: short loops generated at
start-up (sums of sines for the engine and motor, filtered noise for tire
squeal, loose-surface noise, road and wind), with pitch and volume driven
by values the core already publishes (engine speed, slip, load, surface,
speed). No recorded samples, so no asset licences, and no change to the
core or the bridge. It is a cue for the driver, not a model of the real
sound of anything, and it has not been tuned by ear.

## D-046 First graphics pass, at the driver's request

The brief said not to spend effort on graphics; the driver has since asked
to start. Kept to what helps judging the car: a sky and a sun with shadows,
a generated grain on the ground mapped in world space (so the ground
visibly moves, which gives a sense of speed), grass beyond the drawn
ground, banded cones, and a bare interior in the seat view (dashboard,
screen pillars, a steering wheel that turns with the simulated one). Still
no art assets and no car models.

## D-047 Sedan assist raised to 0.40

Second drive: the sedan's steering was still a bit heavy at 0.25. Raised to
0.40. Its power was left alone: the driver felt it "should maybe be
faster", but it already meets its placeholder acceleration and top-speed
targets (0-60 mph in 3.26 s), and the drive was done without sound and
with a featureless ground, both of which hide speed. To be judged again
with D-045 and D-046 in place.

## D-048 Sedan power raised

With sound and the new ground in place the driver still wanted the sedan
quicker, expecting the fastest cars of its class to reach 60 mph in just
under 3 s. Motor torque went from 240 + 420 to 280 + 480 N.m and power
from 137 + 220 to 160 + 250 kW; the simulated time fell from 3.26 s to
2.98 s. The validation target moved from 3.4 s to 2.95 s. Both the car
values and the target are a driver's expectation for a fictional car, still
marked placeholder, not a measurement of any real vehicle.
