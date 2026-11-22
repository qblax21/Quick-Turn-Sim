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

## D-049 Elevation and banking as a profile along a path

Replaces D-015. A track may carry a `terrain` section: heights and banking
angles at distances along one path. Within a corridor around the path the
ground is exactly the road surface (profile height plus cross slope); over
a blend width it merges into smooth terrain interpolated from the elevation
points, and beyond that it is that terrain alone. This was the simplest
form that gives a designed road with crests, dips and banking, stays a pure
function of position (determinism), and needs no heightfield assets.

The ground query now returns a unit normal. Each wheel and each body
contact point treats the ground as a plane through the point below it:
load acts along the normal, tire forces lie in that plane, and camber is
measured against it. On level ground this reduces to the previous model.
Tests: a car in neutral rolls back down a 10 percent grade at about
g sin(theta), a braked car holds on a cross slope, and both cars lap the
circuit on the road without touching the ground with the body.

Known limits: the plane approximation ignores curvature within the contact
patch; non-neighbouring parts of the path must be further apart than twice
the corridor plus blend (checked by a test for the shipped circuit); the
profile is level at both ends, so a lap must start and end level.

## D-050 The circuit is fictional, not a real one

The driver suggested three well-known real circuits. The project rules say
no licensed content and no fabricated real-world data, and both apply: a
real circuit's name and likeness are licensed in commercial sims, and with
no survey data any "recreation" would be invented geometry under a real
name. Ridgeline Circuit is an original 2.85 km layout that borrows only
general ideas found on many circuits (a banked carousel, a climb, a blind
crest into a steep downhill sequence). A real circuit can be added later
as a data file if survey data and permission exist.

## D-051 The ghost is a recording of poses, not a replay of inputs

The best timed run is shown as a translucent car on later runs. The host
records the car's pose every 20 ticks through each run and keeps the
fastest; the ghost file (one per car and track, in the user folder) holds
those poses. Re-simulating the best lap from its inputs would be exact,
but it doubles the physics cost and the file would stop working whenever
the physics or the data changed. The recorder only reads the session, so
it cannot affect determinism. Any completed run counts: there are no track
limits yet, and cone penalties are not applied.

## D-052 Player settings live in the front end's user folder

Display, sound, view, last menu choices and force-feedback adjustments per
preset are stored in `user://settings.cfg` by the front end. Force-feedback
values are still applied by the core; the front end only remembers what the
player changed and passes it on after the preset loads. Preset files in
`data/ffb` are never written.

## D-053 Packaging, with a fallback when export templates are missing

`tools/package.ps1` produces a standalone folder. The proper path is a
Godot export, which needs the export templates (a download of about 1 GB).
Without them the script packs the project and uses a copy of the Godot
editor binary as the runtime, which the engine's licence allows and which
plays identically, so the game can be handed to someone without installing
anything. That fallback is a development convenience, not a release. The
front end finds `data/` beside the executable first, then beside the
project.

## D-054 Traction control, and why the tires felt slippery

Driver feedback: the tires feel slippery, most of all under power. Three
causes were found, none of them inflation pressure (the pressure terms are
zero at the placard pressure both tire files use):

1. Neither car had traction control. The sedan's motors can exceed the
   grip of all four tires at low speed, and the validation launch only
   reached its time because the harness itself backed the accelerator off.
   A driver who floored it got wheelspin.
2. Past its peak the longitudinal curve fell to about 60 percent of peak
   force, so a spinning tire lost far more grip than a road tire does.
3. Sessions start with the tires at road temperature, where the grip
   window gave about 7 percent less grip than at the 50 to 60 degC the
   validation runs use.

Changes: an optional `[traction_control]` table per car (one torque
fraction for the whole car, steered toward a target drive slip, the same
controller shape as the anti-lock system; absent means not fitted). All
three cars have it. The shape factor `pcx1` went from 1.60 to 1.50
(sliding force about 75 percent of peak), the grip window is now centred
on 45 degC and 60 degC wide, and peak friction went up 5 to 6 percent
because skidpad, braking and launch were all on the low side of their
targets. All of these are still placeholders. The validation launch now
floors a car that has traction control instead of using its own limiter.
There is no switch to turn the system off yet. A per-axle controller would
suit the dual-motor car better; this is the simplest thing that works.

## D-055 Car bodies are generated in code

The project owner wants all art made in the project. Car bodies are built
by `godot/scripts/car_body.gd` from the dimensions the core reports: a
table of cross-sections from nose to tail, joined into flat-shaded panels,
with wheel wells cut at the two axles, a glass greenhouse, lamps, mirrors
and spoked wheels. Two shapes exist (two-box below 4.3 m long, three-box
above), both original and generic. The body is single-sided, so the
driver's-seat view looks out through it without hiding anything. This is a
stylised low-poly look, not a detailed model.

## D-056 Trackside cues and the map

To make distance and speed easier to judge, the front end draws painted
edge lines on paved path regions, distance boards 150, 100 and 50 m before
each corner of more than about 30 degrees that follows a straight of 100 m
or more (on the outside of the corner), and conifers beside roads through
open country, placed from a fixed seed. The corners are found from the
path geometry, so any new track gets boards without extra data. None of
this exists for the physics: a tree cannot be hit. A north-up track map
shows the path, cones, gates, the car and the ghost; N switches it.

## D-057 Balance and steering weight after the second drive

Driver feedback: the hatch is a little understeery, the sedan a little
snappy at the limit though it holds a slide well, and both could steer a
touch heavier. Hatch: front anti-roll bar 14000 to 12500 N/m, mechanical
trail 0.022 to 0.024 m. Sedan: lift-off regeneration 0.25 to 0.18, rear
toe-in 0.002 to 0.003 rad, steering assist 0.40 to 0.36. A more forward
torque split (0.44) was tried for the sedan and dropped: with one
traction-control torque fraction for the whole car it cost 0.17 s to
60 mph. All of this is tuning by feel on placeholder data.

## D-058 A lap needs most of a lap's distance

A start/finish gate used to end the run on any second crossing, so a spin
or a reverse over the line set a lap time of a few seconds and that became
the best lap and the ghost. The session now adds up the distance covered
in the run, and a start/finish gate only ends a run that has covered 80
percent of the first path's length; a shorter one simply starts again.
Separate start and finish gates are unchanged. Ghost files saved before
this change can hold such a lap and have to be deleted by hand.

## D-059 Rubber on the fast line, and three map modes

The front end lays dark rubber along an approximate fast line through
each paved region: the line of least curvature inside the track's width,
found by relaxing lateral offsets coarse to fine. It is a drawing aid, not
an optimal-lap solution, it ignores elevation and speed, and it does not
change grip. The track map now has three modes, cycled with N and
remembered: the whole track north up, a zoomed view north up, and a
zoomed view with the car's heading up.

## D-060 Solid trees and barriers

Trees and barriers are now part of the track in the core, because the
front end may not hold anything the physics depends on. A track file can
have a `[trees]` table (trees scattered beside every path from a seed) and
`[[barriers]]` entries (a barrier at a fixed offset from part of a path).
The front end draws what the core reports.

Contact is the simplest model that stops the car: the body is a rectangle
seen from above, a tree is a circle and a barrier a row of thick line
pieces, and each contact is a rigid impact in the ground plane (an impulse
that removes the closing speed with 15 percent bounce and sliding
friction 0.4, plus a shift out of the overlap). It acts on the whole
car's velocity and yaw rate. There is no deformation, damage, roll or
pitch from an impact, no force through the suspension, no sound and no
force-feedback kick, and height is ignored. The restitution and friction
are guesses.

Trees stand 22 m or more from the centre of the circuit (16 m from the
road edge) and 11 m from the centre of the gravel stage. The circuit has
barriers either side of the start straight only. The soak test asks for
less distance on tracks with trees, because its driver never reverses and
ends up held against one.

## D-061 Worn-in rubber and live tire marks

The rubber on the fast line was two clean strips. It is now a wide band
that fades out at its edges, darker under the two wheel tracks and in the
corners and uneven along its length, with single darker streaks of
different lengths on top, placed from a fixed seed. The car also leaves
its own marks while a tire slides (slip ratio above 0.18 or a slip angle
above about 7 degrees, from the values the core reports); the last 6000
pieces are kept for the session. Both are drawing only: the track does
not gain grip where rubber lies. The slip thresholds are guesses.

## D-062 Barriers round the circuit, and hits you can hear and feel

Ridgeline Circuit now has a barrier on the outside of every corner, 13 m
from the road centre (7 m of runoff), from 15 m before the corner to 15 m
after it, as well as the pair along the start straight. The track map
draws them.

The session reports each hit on a tree or barrier with a closing speed
above 0.5 m/s: a running count and the impulse of the last one. The front
end plays a synthesised thud, louder for a larger impulse. The same hit
adds a short kick to the steering-column torque that goes into the
force-feedback pipeline, up to 12 N·m, toward the side that was hit,
dying away with a 70 ms time constant. The kick is an effect, not
physics: a real impact reaches the steering through the wheels and
suspension, which the flat contact model does not represent. The report
is not part of the simulated state and nothing in the physics reads it.
All the numbers are guesses and none has been felt on a wheel.

## D-063 Time against the best lap, and a traction control switch

While a run is timed the HUD shows the time gained or lost against the
best lap at the car's present position: how long this run took to get
here, less how long the best lap took to reach the nearest point of its
own line. The recorder follows the best lap's poses forward as the car
goes round, so it needs no path and works on the cone course as well as
the circuit. No figure is shown more than 40 m from the line the best lap
took. It compares positions, not distance along a centre line, so a very
different line through a corner shifts it slightly.

Traction control is now a session assist like the automatic clutch and
gears, on by default, with a tick box in the menu. It only does anything
on a car that has the system.

## D-064 A run counts only if it took the course

Before this, a run counted if it crossed the finish line, with only a
distance rule on start/finish lines. Saved ghosts showed what that let
through: an 11.7 s "lap" of Ridgeline from rolling over the line, and on
the cone course two runs (the best ones for both cars) that skipped about
80 m of the course by driving straight from the hairpin to the last
straight.

A track can now have `checkpoint` gates. A run counts only if it crosses
every checkpoint forward and in order, covers 80 percent of the timed
length, and has an average speed below 100 m/s. All gates now work only in
the direction of travel, so reversing over a line starts and ends nothing.
A run that fails is counted as invalid, sets no time, does not become the
ghost, and the HUD says "lap not counted" for a few seconds. Checkpoints
are placed where no other part of the course comes within their width:
Ridgeline has three (12 m half width), the cone course two (10 m) and the
gravel stage two (12 m).

A ghost file is checked against the same rules when it is loaded, by
running its recorded positions through the checkpoints, and when a new
best lap is recorded. A file that fails is ignored and is overwritten by
the next valid best lap. On the development machine three of the four
saved ghosts fail and are now ignored: the 11.7 s Ridgeline sedan lap and
the two cone-course runs that cut the course.

Checkpoints do not judge track limits: a car can still cut a corner by a
few metres. Proper track limits (wheels off the road) are not done.

## D-065 Ridgeline graphics pass, and kerbs that exist for the tires

Everything is still generated in code. The sun is lower (36 degrees) and
warmer, with four shadow cascades out to 260 m instead of one to 140 m; the
sky has a sun disc and a thin layer of generated cloud; tone mapping is
ACES with a little glow and aerial perspective in the fog. The terrain has
per-vertex colour: paler and drier higher up, darker in hollows, browner
where it is steep, with broad darker patches from low-frequency noise.
Beside the paved road the grass is drawn as mown bands out to the
barriers. Vertex colours are now treated as sRGB, which they are; before,
everything drawn with them came out too light.

Kerbs are real geometry in the core, not paint: a `kerb` region beside a
path raises the ground the tires roll on, ramping up from the road edge
over 30 percent of its width, flat on top and dropping back over the last
15 percent. Ridgeline has 28, 1.2 m wide and 40 mm high, on the inside of
every corner around the apex and on the outside at the exit. The front end
draws them from heights it asks the core for, so what is seen is what the
tires meet. They use the existing `kerb` surface, which the force-feedback
kerb channel already listens to.

For realism: the kerb raises the contact height but does not tilt the
contact normal, so a tire on the ramp is pushed straight up rather than
also sideways; the height, profile and the kerb surface's grip are
placeholders, and the kerb surface is still marked untuned.

Cost: with vertical sync off, the driver's-seat view on the development
machine (RTX 3060) went from about 540 to about 430 frames a second. The
physics thread kept 1000 Hz with a worst scheduling wake of 2 ticks and no
dropped time before and after, because rendering and physics run on
separate threads.

## D-066 Software transcendental math: replays now reproduce in every binary

Building replay playback showed that a replay recorded in the game did not
reproduce in the command-line tool, nor in the game's own playback, from
the first stored hash onward, although every binary came from the same
source and build. The cause was `fm`, which forwarded `sin`, `cos`, `exp`
and the rest to the platform math library: different binaries, and code
running on Godot's main thread against the simulation thread, did not get
the same bits back. Plain `hypot` calls in the session had the same
problem.

`fm` now calls the `libm` crate, a pure-Rust port of musl's math library,
pinned to one version, for every transcendental function and for `hypot`.
Add, subtract, multiply, divide and square root were already exact. After
the change a replay recorded in the game reproduces exactly in the game's
playback and in `qts-sim replay`, and the test binary, the tool and the
game library compute the same state hash after the same ticks. A test
fails if a transcendental `f64` method appears in the simulation path
again.

This is step 2 of the cross-platform plan in ARCHITECTURE section 5.
Bit-identical results across operating systems are now likely but not
claimed: nothing compares hash sequences between Windows and Linux yet.
Numbers moved only in the last bits; validation results are unchanged to
the precision reported.

## D-067 Replay playback by re-simulation

Every session is recorded from its start, and the recording is saved in the
replays folder when the session ends (sessions under 10 s are not kept,
and only the newest 20 are). F3 saves a copy of the session so far to the
recordings folder; it no longer starts and stops recording. The menu lists
both folders under Replays.

Playback uses the existing replay files: the core re-simulates the
recording once, a slice each frame, keeping a pose and a few display
values every 10 ms, so watching, pausing and scrubbing in either direction
only read stored samples. The stored state hashes are checked on the way;
a replay that no longer reproduces (made by another build, or before a
physics change) still plays, with a notice saying from when it stops
matching what was driven. A replay whose car or track files have changed
since is refused, as before.

Cameras: trackside (posts every 70 m along the track, 17 m to the side,
zooming to keep the car a constant size), chase, orbit (turns by itself or
by dragging) and driver's seat. H hides everything on screen; H also hides
the HUD while driving.

## D-068 Demo flow: title, selection, pause menu, results

The game now opens on a title screen with a camera flying round Ridgeline
behind it (the bridge can load a track for drawing without a session).
Drive leads to the car and track selection, which is the old menu without
its other buttons. Esc while driving pauses the simulation and opens a
pause menu: resume, restart (a fresh session with the same choices; the
session so far is kept as a replay), end the session with results, or quit
to the title. P still pauses without a menu.

The results list every lap or run the core counted in the session, in
order, with the time the core measured and the cones hit, mark the best,
and give the best ever for that car and track and the number of laps not
counted. The front end only notes each count as the core reports it; it
times nothing itself.

## D-069 Force-feedback tuning: meter, minimum force, lap logs

Tuning a preset needs to see what the wheel is asked for, not just what it
is sent. The host now reports the largest request over the last two
seconds next to the clipping share, and F4 draws both against the
soft-clip knee and full scale while driving. Gain and minimum force can be
nudged from the keyboard while the meter shows; changes are kept per
preset like those on the Controls screen.

Minimum force is the usual remedy for a weak or belt-driven base whose own
friction swallows small torques: any non-zero output is raised to
`m + (1 − m)·|x|`, faded in linearly over the first 1 % of the scale so the
output stays continuous through zero and the wheel does not buzz about the
centre. It is applied after the soft clip and does not count as clipping.
It changes only the device command. **Fidelity cost:** it compresses the
difference between small torques (on-centre feel) by the same factor, so
it defaults to zero and is capped at 0.3.

An FFB log (F5) records the next full lap from the start line to the tick
before the finish at the physics rate: the physics column torque and every
pipeline stage, the device angle, and front slip angles, lateral forces,
aligning moments and loads, with the preset written in the header. Rows
are kept in memory and written by a separate thread when the lap ends so
the physics thread never waits on the disk. A reset discards the lap and
waits for the next. `qts-sim ffb-report` summarises a log: clipping
stretches by lap distance, the 99th-percentile request, and mean column
torque and output in 1° bands of front slip angle, which shows whether the
fall in torque past the tire's peak (the cue that the front is at its
limit) survives to the wheel or is flattened by clipping.

The physics is not changed for any of this.

## D-070 Wheel, pedals and shifter read on the physics thread

Before, every control reached the physics through Godot: read once per
frame, handed to the sim thread, and spread over the ticks until the next
frame so it did not arrive as a staircase. That costs a frame of hand-over
plus the age of Godot's once-per-frame device poll.

Now the mapped controls are read from their devices by the sim thread at
the start of every tick (DirectInput, opened shared so the force-feedback
device can be pushed at the same time) and written into the tick's input
frame; unmapped controls keep the value Godot sent, so keyboard and gamepad
work as before. The frame is what replays record, so recordings reproduce
exactly however the input arrived (tested by re-simulating a host
recording made with a device-driven throttle and wheel).

Mapping stays on the Controls screen: assigning watches both Godot's view
of the devices and the core's, and maps every way the movement was seen.
DirectInput axis indices do not match Godot's, so mappings made before
this change are kept for the per-frame path but need assigning once more
to be read directly.

The host measures, each second: the per-frame hand-over (Godot handing the
value over to the physics having all of it), the time from the device read
to the end of the tick, and, while a wheel is mapped both ways and moving,
the lag of Godot's steering behind the device's by matching the two
signals (2 s window, 0 to 150 ms searched, median of recent estimates).
That last figure is the complete one and needs a person at the wheel.

Measured here (60 fps vsync, about 430 fps without): hand-over 15.9 and
1.5 ms mean before, device read to physics 0.09 ms after. A read costs
about 1 microsecond (median), 4 at the 99th percentile.

## D-071 Car setups as changes to the data file, carried by recordings

A setup is a list of `key = value` changes to the car and tire files:
cold pressures, camber, toe, springs, bump and rebound, anti-roll bars,
brake bias, and for cars that have them the limited-slip differential and
the gear ratios. Keys a car does not offer and values outside the garage
range are refused, not clamped, so a setup file always means what it says
and a hand-edited one cannot put the model outside what a real car could
be. The ranges are in `docs/SETUP.md`; like the car values, they are
placeholders.

Brake bias keeps the total brake torque and moves its split. Pressures
are gauge and become the tire's cold absolute pressure. Gear ratios must
stay in order.

A setup changes parameters, not state, so it is not in the snapshot and
has to travel separately: the replay header (format 2) and the ghost file
(format 2) store the canonical text, and `Session::for_replay` rebuilds a
recording with it. Older files read as the car as delivered. A test
re-simulates a recording made with a setup exactly, and shows the same
inputs drive differently without it.

`tests/setup.rs` moves every adjustment of every car to the far end of its
range and requires the drive to change; pressure, camber, toe, anti-roll
bars, brake bias and every gear ratio are also checked against their
expected effect (for example, engine speed over wheel speed changes by
exactly the ratio).

The best lap stays the best lap for the car and track whatever the setup;
the HUD and results say which setup it was driven with.

## D-072 Track limits, and kerbs that push sideways

**Track limits.** A track may name the road a lap must stay on
(`[track_limits]`: a path and a half width); kerbs beside it count as
track. Each tick of a timed run the four wheel centres are checked; once
all four have been beyond the road and kerbs for 20 ms the run is marked
and will not count at the line (it adds to the laps not counted, like a
missed checkpoint). The 20 ms only stops a single-tick graze from
counting; any real excursion lasts far longer. Ridgeline has limits at
the 6 m road edge. The cone course and the gravel stage have none: their
checkpoints and cones are the rule there. The new timing fields are part
of the snapshot, so replays recorded before this change report that they
no longer reproduce.

**Kerb normals.** Before, a kerb raised the ground under the tire but the
contact normal stayed that of the road, so climbing a kerb only lifted the
tire. Now the kerb's slope tilts the normal: the load acts along it, so
the ramp pushes the tire back toward the road as well as up, the tire's
force plane and camber follow the ramp, and the kingpin moment (which
already used the full contact force) carries it to the wheel. A tire does
not feel a sharp edge at a point; the slope is averaged over 0.2 m, about
a tread width, which also starts the tilt half a tire before the edge.

**Kerb ends.** The kerbs used to start and stop with a 40 mm vertical
step along the road. A tire meeting that step end-on gave a 13 N.m swing
at the column in 30 ms, the largest kerb event in the model and not one a
real kerb gives. Kerbs now taper over 1 m at each end, and the taper's
slope tilts the normal too.

**Size and grip.** Height 40 mm, width 1.2 m, ramp 1:9, kept: club-circuit
kerbs are typically 25 to 50 mm high and 1 to 1.5 m wide. Grip 0.85 of
tarmac kept (dry painted concrete); surface texture raised to 2 mm at
0.3 m. All placeholders, from general knowledge, not measurement.

**Force feedback before and after** (`qts-sim kerb-ffb`, Tern GT with the
right wheels over the T1 inside kerb at about 18 m/s, CSL Elite preset;
chart in `docs/img/kerb_ffb.svg`): the swing in column torque climbing on
fell from 14.8 to 4.9 N.m, the output no longer drops to zero for an
instant on entry or spikes on exit, and the steady torque on the kerb is
unchanged (12.1 and 12.6 N.m). The sideways push itself is small at the
column, about 0.5 to 1 N.m while a front tire climbs the ramp, which is
what 450 N of push through 24 mm of trail and a 14.5:1 rack gives. The
kerb is felt mostly through the road-detail channel, as before.

## D-073 H-pattern shifter, analog clutch, grinding and stalling

**Shifter.** Each gate of an H-pattern shifter is mapped to a button on
the Controls screen (gates 1 to 6 and R); the lever's gate, or neutral
between gates, is sent every tick as the gear request. Read on the physics
thread when assigned that way, so a shift reaches the gearbox within a
tick. While the lever position is known, nothing else shifts the box:
sequential buttons and automatic gears are ignored. Sequential shifting is
unchanged and stays the default.

**Grinding.** With the automatic clutch off, a gear goes in only with the
clutch pedal at least 85 % down, or with the engine already within about
150 rpm of the speed that gear would turn it at (a rev-matched clutchless
shift). Otherwise the synchroniser cannot bring the engine round: the box
stays in neutral, the HUD says so and a grinding sound plays, until the
clutch goes down or the lever comes out. Coming out of gear needs nothing
and is immediate. There is no damage model.

**Stalling.** This replaces the implicit stall of D-020. A running engine
dragged below 40 % of idle speed stalls: no combustion torque and no idle
control, only friction. It fires again once turned above 25 % of idle by
the starter, or above 60 % of idle by the wheels (a bump start); the gap
between 40 % and 60 % stops a dragged engine flickering between the two.
The starter (I, or a mapped button) gives 80 N.m at rest falling to
nothing at 35 % of idle, and works only with the clutch down or in
neutral, like the interlock on a road car. With the automatic clutch on,
the starter turns a stalled engine by itself and the clutch never lets the
engine stall, as before.

**Clutch.** The pedal is analog and calibrated like any axis.
Engagement stays linear in pedal travel; a free-travel and bite-point
curve is not modelled (flagged in STATUS).

Stall and grind state are part of the snapshot. Tests: stalling in gear,
the starter interlock, restarting, grinding without the clutch, a
rev-matched shift back into second, refusal of a gear far too tall, and a
bump start from rolling in second.


## D-074 Clutch pedal curve, over-rev and drivetrain FFB cues

**Pedal curve.** Clutch engagement is no longer linear in pedal travel
(replacing the clutch paragraph of D-073). Each car's `[drivetrain.clutch]`
gives `free_travel`, `bite_point` and `bite_width` as fractions of pedal
travel. Through the free travel the clutch is fully clamped; from there it
falls linearly to half of its capacity at the top of the bite zone, then
quadratically across the zone to nothing, reaching zero with zero slope so
the first touch is gentle; below the zone, down to the floor, it is free.
Half capacity at the top of the zone is chosen because a clutch rated at
1.5 times engine torque then already holds the engine there, so the zone
is where the car is driven on the clutch. The curve is a function of the
car parameters (`ClutchParams::engagement`), so the Controls screen draws
it from the physics code through the bridge instead of copying it.

The bite point (30 to 65 % of travel) and width (10 to 40 %) are garage
adjustments, as a cable or master-cylinder pushrod adjustment would be;
they travel with setups, replays and ghosts. Pedal calibration stays the
axis calibration it was: released reads 0, floored reads 1.

A gear now goes in cleanly when the clutch is free (below the bite zone),
not at a fixed 85 % of travel; the starter interlock keeps a switch at
85 %, near the floor, as on a road car.

**Rev limiter and over-rev.** The fuel-cut limiter at `redline_rpm` was
already there and stays. It cannot stop the wheels spinning the engine
through the clutch, so a missed downshift drives the engine past it. Each
engine has `overrev_damage_rpm` and `overrev_failure_rpm` (placeholders:
7,500 and 9,000 rpm for the Tern GT). The highest engine speed since the
last reset is kept and shown on the HUD; above the damage speed the HUD
warns with the peak reached. With the new "Engine damage from
over-revving" option (off by default; part of the assists, so in the
snapshot and every replay), time above the damage speed accumulates
damage at a rate rising with the square of how far into the band the
engine is, up to failure at the failure speed. Damage costs up to 60 % of
full-throttle torque; a failed engine makes no torque and cannot be
restarted until the car is reset. These numbers are placeholders: real
valvetrain failure is not a deterministic function of speed and time, and
nothing here is measured.

**FFB cues.** Two detail channels join road and kerb detail: clutch
shudder (11 Hz, the order of a front-drive driveline's first torsional
mode) and gear grind (37 and 53 Hz together). The drivetrain reports
clutch slip, clutch torque against capacity, and the speed a refused gear
failed to match; `host::drivetrain_cues` turns those into intensities, and
the pipeline adds a vibration of that intensity times a per-preset
amplitude (`clutch_shudder_nm`, `gear_grind_nm`; 0.3 and 0.4 N.m in the
shipped presets, sliders on the Controls screen) inside the existing
detail limit. They are cues, not column physics: a real car passes
clutch judder mostly through the floor and pedal and gear clash through
the lever, so they are kept light and can be set to zero. Shudder is
strongest when the clutch carries a large share of its capacity while
slipping slowly, and fades once it locks or slips fast; the shape is
judgement, not measurement.

Tests: the pedal curve's shape and continuity; nothing driven past the
release point; slip in the bite zone; lock in the free travel; a moved
bite point moving where the car pulls; the limiter holding a free-revving
engine without damage; a missed downshift past the damage speed logged
with damage off; failure with damage on, no restart, repair on reset; the
cue channels' amplitudes and the detail limit; and a manual-clutch launch
plus missed downshift replaying exactly with damage on and off. Replays
that used the clutch pedal before this change no longer reproduce, since
the pedal now acts through the curve.

## D-075 Steering system: speed-sensitive assist, rack friction and inertia, gyroscopic and weight moments

The fixed `assist_fraction` of D-042 (the share of the steering load the
power assist carried) is replaced by a model of the whole steering system,
all in the physics; the force-feedback pipeline is unchanged, so any
difference at the wheel comes from here.

**Assist.** A boost curve, as electric power steering works: below
`assist_threshold_nm` of driver torque the assist does nothing, so the
centre feels the unassisted rack; above it the assist adds its gain times
the driver's torque beyond the threshold, so the driver holds
`(|load| + K·T0) / (1 + K)`. The gain K comes from a per-car table against
speed (`assist_speed_kmh`, `assist_gain`), linear between points. The
Tern GT runs from 6 when parking to 0.9 at 160 km/h, the Halden E4 from 7
to 1.1; both are placeholders chosen to land near published orders of
magnitude for EPAS cars (about 5 N.m at the rim when parking, 3 to 4 N.m
in a half-g corner at 100 km/h), not measurements. Real boost curves are
non-linear in driver torque; linear above a threshold is the simplest
shape with an on-centre zone.

**Friction and inertia.** The column's rate and acceleration come from the
driver's steering angle, filtered at 20 Hz (it arrives quantised at the
physics rate). Rack friction (`rack_friction_nm`) opposes the turning
direction, smoothed through zero over about 0.15 rad/s; reflected inertia
(`steering_inertia_kg_m2`: road wheels about their kingpins and the rack,
over the ratio squared, not the steering wheel the driver holds) opposes
the column's acceleration. Both are felt in full, not assisted. The angle
is the driver's input, not a simulated rack position, so a scripted driver
that steps the steering produces impossible accelerations; the
comparison command limits its steering to 12 rad/s and 300 rad/s², as
hands on a wheel would.

**Gyroscopic moment.** Each front wheel's spin angular momentum is turned
by the body's rotation and by camber change from suspension travel; the
steering supplies Ω × H to the wheel and the wheel pushes back on the
kingpin with the opposite moment. Turning the wheel about its own kingpin
adds nothing about that axis. In a steady corner this is centring and
proportional to caster, speed and yaw rate; over a kerb strike the camber
rate gives a sharp kick. On a scripted Ridgeline lap it is 0.2 to 0.4 N.m
at the column most of the time, with kerb-strike peaks of 4 N.m (Tern GT)
and 11 N.m (Halden E4); those peaks are not validated.

**Weight moments.** The vertical tire load is taken out of the
trail-and-scrub lever arm (which, rotated about the vertical rather than
the kingpin, gave a small decentring moment from caster that a real
geometry does not), and put back as Gillespie's moments: kingpin
inclination lifting the car (`-(Fzl + Fzr)·d·sin λ·sin δ`, centring) and
caster with weight transfer (`(Fzl − Fzr)·d·sin ν·cos δ`, centring in a
turn), d being the scrub radius. With the small scrub radii of these cars
both are small; a geometry with the spindle offset would be the next step.

The car exposes the parts (`CarOutput::steering`). `qts-sim
steering-compare` runs a scripted Ridgeline lap and the validation
skidpad on both cars with the old model (a hidden `set_legacy_steering`
switch with the old assist shares) and the new one, and
`docs/img/steering_torque.svg` shows the result. On the skidpad the column
torque per g falls from about 27 to 31 N.m/g to about 10 to 13 N.m/g, and
rises progressively with speed where it used to be linear in lateral
acceleration. The force-feedback gain of the presets was set against the
old, heavier torque and will need raising; that belongs to the CSL Elite
retune from the lap logs.

Tests: the boost curve (threshold, continuity, speed dependence, never
more than the load); the assist sharing a cornering load; friction
opposing both directions of turning; inertia equal to J·α under a steady
acceleration; no gyroscopic moment at rest and the steady-corner value
within 25 % of 2·I·ω·r·sin(caster); the kingpin-inclination moment
centring a steered car. The gravel soak's random driver now runs seed 5:
seed 1 pinned the car against a tree at 474 m with the new steering;
over seeds 1 to 6 the distance ranged 452 to 2473 m before and 474 to
1990 m after.

## D-076 Brake temperature and fade; tire wear

**Brakes.** Each disc is one thermal mass (`[brakes.thermal_front]`,
`[brakes.thermal_rear]`, per wheel). It takes `disc_heat_share` (0.9) of
the brake's friction work, |brake torque × wheel speed|, so a locked
wheel heats the tire, not the disc. It loses heat by convection that
grows linearly with road speed, scaled up by a duct
(`1 + duct_cooling_gain · duct_opening`), and by radiation
(Stefan-Boltzmann with emissivity times area). Pad friction follows a
per-car table against disc temperature (`fade_temp_c`,
`fade_friction_factor`): slightly low cold (0.92 at 0 degC, 0.94 at a
28 degC start), flat through the working range, fading above about
450 degC to 0.45 at 800 degC; the service brake and handbrake torque
scale with it, and anti-lock works on top. Disc heat capacities follow
from disc sizes and cast iron (estimated); cooling is set for a time
constant of about two minutes at 30 m/s; the fade curve is the shape of
a road pad, not a measured pad. One mass per disc ignores the gradient
through the disc and the pad temperature, which is what really sets
pad friction; brake fluid boiling is not modelled.

Garage: front and rear brake duct opening, 0 to 100 %; the cars come
without ducts. A fully open duct doubles the speed-dependent cooling, a
placeholder.

**Tire wear.** Each tire's `wear` (0 new, 1 at the end of the usable
tread) grows with the energy dissipated by sliding in the contact patch,
an Archard-type law (`[wear] rate_per_mj`). A scripted Ridgeline lap at
about 0.7 g puts about 0.5 MJ through the busiest front tire; the rate is
set so that lap costs a quarter of a percent, a few hundred laps of tread,
fewer when driven harder. Wear lowers friction by up to
`grip_loss_at_full_wear` (8 %), standing for compound ageing and graining:
a shallower tread alone grips as well or better in the dry, so this is
not tread depth. It raises slip and cornering stiffness by up to 15 %
(less tread to squirm) and takes away up to half the heat capacity of the
tread surface and core nodes, so a worn tire heats and cools faster.
Wear never recovers within a session; resetting the car fits new tires
and cold brakes, as it repairs the engine.

Both are state (snapshot, replays) and are shown on the F1 overlay (wear
%, disc temperature and pad friction factor per wheel) and in telemetry
(`*_brake_temp_c`, `*_tire_wear`).

Tests: a disc's stored heat equals the heat put in less the heat cooled
away, to 1e-6; a full stop from 30 m/s puts 75 to 98 % of the lost kinetic
energy into the brakes (the rest into tire slip under anti-lock, rolling
and drag); fifteen hard stops from 40 m/s without rest take the front
discs past 550 degC and cut the deceleration by more than 10 %, with the
rears cooler; discs cool faster at speed than parked, and faster again
with a duct; a duct setting reaches the physics; wear equals the sliding
energy times the rate; a worn tire has the configured peak-grip loss,
higher stiffness, and stores exactly the heat put in with its thinner
tread while warming faster.

## D-077 Validation against published figures of comparable real cars

At the project owner's request the placeholder targets are replaced by
published instrumented road-test figures of the closest real cars: the
seventh-generation Volkswagen Golf GTI (manual, Performance Pack) for the
Tern GT, and the 2018 to 2020 Tesla Model 3 Performance for the Halden E4.
This is the one place real names enter the project: `data/validation` and
`docs/VALIDATION_SOURCES.md`. The cars, their data files and the game stay
fictional, as before. Targets are labelled `estimated`, not `measured`:
they measure the comparable car, not ours.

Sources are Car and Driver test data (read as syndicated on Yahoo Autos;
Car and Driver's own site and Edmunds could not be read from here) and
Volkswagen's press figures for top speed, since the US test cars were
governed. Every value cites its article, author and date.

To compare like with like, two metrics were added to the harness instead
of converting the published figures: `braking_distance_70_0` (Car and
Driver brakes from 70 mph) and `accel_0_60_rollout` (Car and Driver
subtracts a one-foot rollout). The skidpad radius differs (30 m here, a
300-ft circle there) and is not corrected; neither source gives an
understeer gradient, so those two targets stay placeholders.

Result, with no physics change: everything within tolerance except the
Halden E4's 0-60, 12 % quick (2.72 s against 3.1 s), a consequence of motor
power raised earlier from driving feedback. Physics changes to close gaps
wait for the owner's approval, so the target carries a new `known_gap`
note: the harness still compares it and prints `FAIL*` with the reason,
but the run (and CI) does not fail on it; if the gap ever closes, the
stale mark fails the run until it is removed. The proposed parameter
changes are listed in `docs/VALIDATION_SOURCES.md`.

## D-078 Validation gaps closed with published specifications

With the owner's approval, the gaps of D-077 were closed by bringing the
cars to the comparable cars' published specifications, not by fitting
parameters to the targets:

- Halden E4: motor peak power 160 + 250 kW to 137 + 213 kW, 350 kW
  together, Car and Driver's estimate of the comparable's output, in the
  same proportion. 0-60 with rollout went from 2.72 s (-12.1 %) to 2.91 s
  (-6.3 %) against 3.1 s; the remaining 0.2 s was not tuned away. The
  `known_gap` mark is removed.
- Tern GT: Volkswagen's torque curve (350 N.m from 1500 to 4400 rpm,
  169 kW to 6200 rpm), the tested kerb weight (1413 kg, inertias scaled
  with it) and a 225/40 R18 summer tire derived by size from the
  performance placeholder, the size the comparable was tested on. Every
  target is now within 3 %.

Tried one at a time, the real weight on the old 205-section tire moved
the hatch away from the targets, which pointed at the tire. The tire's
compound coefficients are still placeholders, so a match here says the
car is in the right place, not that the tire is right.

The changes alter the hatch's feel (heavier, more torque, more grip) and
the sedan's launch; earlier drives that set its steering and balance were
on the old values. The gravel soak's random driver now resets the car
after 15 s without moving, as a person would, instead of searching for a
seed that does not pin it against a tree; seed 1 is back.

## D-079 Force feedback in real units; wheel-base models

The pipeline already worked in newton-metres (output = gain × column
torque ÷ the base's peak torque); this makes that explicit and separates
the hardware from the feel. `data/wheels` holds wheel-base models with a
sourced peak torque and a `verified` flag. Picking one on the Controls
screen sets the peak torque, which stays adjustable for any other base.
Gain is now described as what it is: N·m at the driver's wheel per N·m at
the car's steering column, the same on every base; gain 1 is the car's
real torque. The Controls screen spells out what hard cornering asks of
the chosen base, and the F4 meter shows column torque, the torque asked
of the wheel and the torque given, in N·m.

Sources. The Fanatec direct-drive figures (CSL DD 5 N·m or 8 N·m with the
Boost Kit 180, ClubSport DD 15, DD+ 18, Podium DD1 20, DD2 25) were read
from Fanatec's support article on 2026-11-20. The CSL Elite's 6 N·m comes
from Fanatec's own product pages as quoted in search results; the base is
discontinued, the pages redirect, and an attempt to read an archived copy
was stopped, so it is marked unverified with a note, here and in the FFB
preset. Other makers' figures were only found in third-party tables and
are not preset; a "custom" model covers them.

Default gain. `qts-sim ffb-gain` drives scripted Ridgeline laps of every
car (hand-limited steering, planned at 0.7 and 0.85 g) on the current
physics and takes the 99th percentile of column torque: 12.5 to 14.8 N·m,
the highest the Halden E4's. The default gain puts that at the soft-clip
knee: 0.75 × peak / 14.8. For the 6 N·m CSL Elite that is 0.30, the same
as the earlier guess (which had been reasoned from the heavier unassisted
torque before D-075); for the 5 N·m generic preset 0.25, from 0.20. The F5
log report now prints the same figure from a real lap, so the CSL Elite
gain can be confirmed from the owner's logs.

## D-080 Tire pressure with temperature: checked, tested and shown

Checked first: pressure already followed temperature. Each tire's
inflation gas is a thermal node heated by the carcass (the thermal
model); absolute pressure follows the gas temperature at constant volume
from the cold setting; and the pressure deviation feeds vertical
stiffness (`pfz1`), slip and cornering stiffness and friction through the
Magic Formula's pressure terms (`ppx*`, `ppy*`), and carcass stiffness,
which sets the relaxation length. Nothing in the physics was changed or
retuned.

What was missing: tests through a running tire and any display. New
tests: two minutes of hard cornering heat the carcass and then the gas,
and the pressure rises by exactly the ratio of absolute gas temperatures;
at a higher pressure alone the tire's characteristics are those of the
Magic Formula at that pressure (in these placeholder files cornering
stiffness at 4 kN goes slightly down, because `ppy2` raises the load of
peak stiffness), the relaxation length shortens and peak friction falls.
The F1 overlay shows cold (as set) and hot pressure per wheel in bar, and
the garage shows the cold settings against the hottest pressures of the
last drive of that car. Tires starting at a road temperature above the
20 degC the cold pressure is set at already read a little high at the
start.

## D-081 Per-lap telemetry export and lap comparison

With "Save telemetry for every lap" on (Settings), or `qts-sim run
--lap-telemetry DIR`, each lap driven is written to its own CSV in the
existing telemetry format (all 116 channels: speed, inputs, rpm, gear,
slip, loads and temperatures per wheel, brake temperatures, wear,
steering torque and the FFB stages) at 100 rows per second, under
`recordings/laps/<car>_<track>_<time>/lap_NNN_<time>.csv`. Lap
boundaries come from the session's own run counters, so counted and
uncounted laps are told apart (metadata `lap_counted`, `lap_time_s`).
The recorder only reads the session after each tick, as the telemetry
writer does, and files are written on a background thread; a test checks
that the state hash is the same with and without it.

`qts-sim lap-compare A.csv B.csv [--svg FILE]` lines two laps up by
distance (speed integrated over time, accurate to about 1 % of a lap),
prints the time difference and the 100 m stretches where B gains or loses
most, and draws speed, throttle, brake, steering angle, steering torque
and the running time delta (`docs/img/lap_compare.svg`: a scripted
Tern GT lap against a scripted Halden E4 lap on Ridgeline). Lining up by
integrated distance drifts slightly when two laps take different lines;
a track-position channel would be the improvement.

The headless `path` driver now laps a closed circuit instead of stopping
at the end of its path.

## D-082 Photo mode and a trailer director

Both live in the front end and only read poses and move a camera, so
neither touches the physics or replay determinism (a test checks that the
replay used still reproduces exactly after an export).

**Photo mode** (F8 while driving, O in a replay): the session or the
replay is paused (and resumes afterwards unless it was paused already),
the HUD is hidden, and a free camera takes over from the current view:
WASD to move, Q/E down and up, Shift faster, right mouse to look, the
wheel or a slider for field of view, and depth of field (Godot's
practical camera attributes) with focus distance and blur amount; the
sharp band widens with the focus distance as a lens's does. Enter or F12
captures through an off-screen viewport sharing the scene's world at 1,
2 or 4 times the window size (up to 16384 px a side), without any
overlay, to `photos/photo_<time>_<w>x<h>.png`.

**Trailer director** (replay viewer): `[` and `]` mark a stretch, D
exports it. The director cuts between five shot kinds in a fixed order
(trackside posts, low chase, slow orbit, side tracking, helicopter) with
shot lengths of 3 to 5 s, and renders the stretch frame by frame at a
fixed rate (30 fps by default) through the same off-screen capture, so
the sequence is smooth whatever the machine's frame rate, to
`trailer/sequence_<time>/frame_00001.png...` with `shots.json` (the cut
list) and a README giving the ffmpeg command for a video. Encoding is
left to ffmpeg; nothing is bundled. The same replay and marks give the
same sequence.

`godot/tests/photo_trailer.gd` drives a session, checks photo mode
freezes it and hides the HUD, captures a 2x still with depth of field,
then exports 2 s of the replay at 15 fps and checks frame count, size,
cut list and replay exactness. It saves its replay to a temporary file
rather than the player's replays folder (where saving trims the oldest)
and removes what it wrote.

## D-083 Steering lock and ratio from published figures; rotation matching and soft lock

**Lock and ratio.** Each car's steering-wheel lock now comes from its
comparable car's published turns lock to lock: 2.1 turns (378 degrees
each way) for the Tern GT, from Volkswagen's description of the Golf GTI
Mk7's progressive steering ("just over two full turns" end stop to end
stop, read on Volkswagen's newsroom; 2.1 turns and 380 degrees in a
release quoted in search results), and 2.00 turns (360 degrees) for the
Halden E4, from the Tesla Model 3 owner's manual (2017 to 2023),
Subsystems, which also gives an 11.6 m curb-to-curb turning circle.
Neither maker publishes a ratio, so it is derived: the road-wheel lock is
the one at which the car, turning about its rear axle line at its mean
front angle, puts the outside edge of the outer front tire on the
published turning circle (10.9 m for the GTI, from Volkswagen's facts
sheet), with this car's own wheelbase, track and tire width; ratio = lock
over that angle: 11.73 (was 14.5) and 10.30 (was 12.0). A first
derivation from the outer wheel's angle under ideal Ackermann gave 12.5
and 10.95; driven, the hatch turned an 11.76 m circle, 8 % wide, because
with partial Ackermann the car turns at its mean front angle. The
progressive rack of the comparable car (slower at the centre, quicker
toward lock) is represented by this constant average; the model has no
variable ratio.

Tests: the mean front angle equals the steering-wheel angle over the
ratio at 5 to 90 degrees, lock over ratio equals the angle from the
turning circle, and driven at full lock at walking pace the cars trace
11.12 m and 11.81 m against the published 10.9 and 11.6 m (tire slip
accounts for the 2 %).

**Rotation matching.** With "Match the wheel's rotation to the car" on
(the default), the physical wheel's angle, from its axis and the rotation
the base is set to (now a slider in degrees on the Controls screen), maps
one to one onto the car's steering wheel: 90 degrees turned is 90 degrees
of the car's, and the car reaches full lock where the real one would.
Done on the physics thread for wheels read there, and through the bridge
(`matched_steer`) for wheels read through the front end; the steering
frames that reach the physics, and so replays, carry the matched value.

**Soft lock.** Past the car's lock the pipeline adds a wall: torque
rising linearly to the base's full torque `soft_lock_full_deg` (3) further
on, plus damping while the wheel keeps going outward. It is added after
the soft clip, so it is never compressed. It needs the wheel's angle,
from the force-feedback device or the mapped steering axis. It is outside
the simulation, like the rest of the pipeline.

The quicker ratios raise column torque: the highest 99th percentile on
scripted Ridgeline laps went from 14.8 to 17.0 N.m, and the derived
default gains from 0.30 to 0.26 (CSL Elite) and 0.25 to 0.22 (generic).

## D-084 Clutch-pack limited-slip differential, open and locked

The differential of the engine car now has three modes. Open; locked (a
spool: both wheels held together, as a constraint that can carry far more
than the tires can give); and a clutch-pack (Salisbury) unit, which
replaces the earlier "locking fraction" parameters. In the clutch pack,
spring preload gives locking torque at no load, and the pinion shaft's
ramps turn input torque `T` into a clamping force `T / (r_p tan θ)`,
which the packs turn into `μ·F·r_c·n / 2` of locking torque, so

    locking = preload + |T| · μ · n · (r_c / r_p) / (2 tan θ)

with the power ramp angle on drive and the coast ramp angle on overrun,
quoted as limited-slip makers quote them: lower locks harder, 90 degrees
gives nothing (a one-way unit). Locking uses the input torque of the
previous tick.

Defaults. The Tern GT stays open: its comparable car, the Golf GTI, has a
front differential lock only as part of the Performance Pack (Volkswagen
facts sheet). The pack and ramp values (preload 50 N.m, 45 and 60 degree
ramps, 8 faces, plate friction 0.12, radius ratio 1) are placeholders in
the range aftermarket clutch-type units are sold with, used by the
`hatch_tern_gt_lsd` variant and when a pack is fitted in the garage; the
GTI Performance's own lock is electronically controlled, not a ramp unit,
so there is nothing published to source them from. With them the drive
side locks 0.48 N.m per N.m of input and the overrun side 0.28. The
Halden E4 has one motor per axle with a fixed open differential each; it
is not given a differential setting.

Garage: type (open, clutch pack, locked), preload 0 to 200 N.m, power and
coast ramps 30 to 90 degrees, for every engine car. Setups and replays
written with the old lock percentages still load: the percentage reads as
the ramp angle that gives it.

Tests: the locking per N.m for given ramps and the conversion back from
percentages; on a split surface with the manual clutch and traction
control off, the locking torque equals preload plus the power-ramp share
of the input torque at a quarter, half and three-quarter throttle (within
3 %) and rises with it; on overrun the coast ramp sets it; open lets the
ice-side wheel spin, locked holds both wheels together, and the clutch
pack holds the ice wheel back.

## D-085 Wheel and pedal setup wizard; pedal curves and deadzones; device profiles

On first launch (no saved controls and the wizard never run; existing
installs are not interrupted), and from a button on the Controls screen,
a wizard walks through:

1. the wheel: turn it, and the axis that moves on any device the core can
   read on the physics thread is taken as steering;
2. the centre: held straight ahead;
3. the rotation: turned exactly a quarter turn; the device reports its
   whole rotation as -1 to 1, so the rotation lock to lock is
   180 / (the axis movement), e.g. 0.2 for a 900-degree base; it feeds
   the rotation matching of D-083, and the steering calibration becomes
   the whole device range about that centre;
4. to 6. each pedal: pressed to the floor and released, detected as the
   axis that moves (not one already taken), its rest and floor recorded;
   each can be skipped;
7. brake curve and deadzones: a response curve (an exponent, 1 linear)
   and a deadzone at each end of every pedal, shown live;
8. force feedback: the wheel-base model sets the peak torque in N·m and
   the gain that puts hard cornering at the knee (D-079, D-083);
9. save: the mappings, shaping, rotation and FFB choice are written as a
   profile for the device (by name) and loaded again when that device is
   found.

Pedal shaping is part of the axis mapping (`PedalShape`): travel inside
the low deadzone reads zero, inside the high one full, and the live
travel between is raised to the curve's power. The physics-thread path
(in the core, with the deterministic `powf`) and the front-end path do
the same arithmetic. What reaches the physics is the shaped value in the
input frame, so replays are unchanged in kind.

Tests: the shaping (deadzones, curve, continuity; steering ignores it);
in the game, the rotation from a quarter turn, the suggested gain, the
same shaping numbers on the front-end side, and a device profile saved
and loaded back. The steps that need hands on a wheel (turning it,
pressing pedals) have not been tried on hardware.

## D-086 Tire sanity plots against published ranges

For each tire file, `qts-sim tire-plots` computes the steady-state Magic
Formula as the simulation uses it (reference pressure, optimum
temperature, tarmac) and draws lateral force against slip angle and
longitudinal force against slip ratio at half, one and one and a half
times the reference load, and the combined-slip curves at the reference
load (`docs/img/tire_<id>.svg`). The same curves (`qts_sim::tire_plots`)
are checked in `tests/tire_sanity.rs` against published ranges, from
Wikipedia's "Tire load sensitivity" citing Milliken and Milliken (1995):
peak lateral force at 6 to 10 degrees of slip angle for production-car
tires, rising with load; the friction coefficient falling with load; the
peak force rising roughly as load to the 0.7 to 0.9 (that last marked as
needing a citation in the article). The longitudinal peak's slip ratio
has no source found, so it is plotted, not judged. Combined slip is
checked against the friction ellipse of the pure peaks in the same
direction.

No physics was changed. Two gaps are recorded in the test and in
`docs/VALIDATION_SOURCES.md`: every file exceeds the ellipse by 3 to 4 %
when braking at about 6 degrees (likely the placeholder `rbx1`, `rbx2`),
and the unused street tire peaks at 10.15 degrees. The test fails if
either gap grows or closes, so the record stays true.

## D-087 The two tire gaps of D-086 closed

With the owner's approval:

- **Combined slip.** `rbx1` raised from 12 to 16 in all three tire files.
  It sets how fast longitudinal force falls away with slip angle; at 12,
  braking at about 6 degrees reached 3 to 4 % beyond the friction ellipse
  of the pure peaks. A sweep confirmed it as the cause (raising `rby1`
  alone also helps but needs a larger change) and found 16 the smallest
  round value that keeps every file inside: now at most 0.9998 of the
  ellipse.
- **Street tire peak angle.** `pky1` raised from 19 to 20: the lateral
  peak at the reference load moved from 10.15 to 9.65 degrees, inside the
  published 6 to 10 degrees (19.5 would have left only 0.1 degree of
  margin). No car uses this tire.

Effect on validation: the Tern GT's skidpad went from 0.934 to 0.923 g
(-1.9 % against 0.94 g) and the Halden E4's from 0.955 to 0.953 g (-0.7 %
against 0.96 g); braking, acceleration, top speed and understeer
gradient unchanged within a millimetre or a thousandth. The skidpad loss
is the expected one: at the limit the tires carry some longitudinal force
too, and no longer exceed the ellipse doing it. Every target still
passes. The tire test now asserts both bounds outright, with no gaps
listed.

## D-088 Fixes from a drive: title camera, steering column, traction control, the sedan's snap

The owner's first drive after D-087 reported four things; each was
measured before it was changed.

**Title camera "bouncing".** Its height never moved (the scenery path is
flat), but its aim snapped from one path point to the next: 0.0047 rad
per frame squared of jerk. The look-ahead point is now interpolated, the
height averaged along the path, and position and aim eased toward their
goals; measured jerk 0.00000007 (`godot/tests/title_camera.gd`).

**FFB kicks out of turns, a bumpy straight.** A hand-limited scripted
Ridgeline lap showed every large kick (11 to 18 N.m at the column within
20 ms) was a kerb strike: an inside front wheel landing hard or leaving
the ground, the tire moment collapsing or jumping at once, plus up to
7 N.m of gyroscopic moment. On straights the steering torque buzzed with
rack friction flipping sign on every small correction, tire load and the
gyroscopic term. All of it reached the driver instantly because the
model handed the rack-side torque straight to the column. A car with
electric power steering does not: the driver feels the twist of the
column's torsion bar, with the assist motor, rack and road wheels behind
it. That is now modelled: the felt torque is the bar's, following the
rack-side torque as a second-order system set by the bar's stiffness
(115 N.m/rad, about 2 N.m per degree), the inertia behind it (0.05 and
0.06 kg.m2 at the column) and its damping (0.7 of critical), all
placeholders, which put its frequency near 7 Hz. Steady load passes
unchanged (to 1 % on the skidpad circle); tick-to-tick roughness on
straights fell about five times (0.034 to 0.007 N.m per ms on the hatch,
0.041 to 0.008 on the sedan) and the largest kerb kicks halved (14.9 to
7.7 N.m, 18.3 to 7.9 N.m in 20 ms). What remains of a kerb strike is the
wheel really unloading. The FFB pipeline is unchanged; the derived
default gain stays 0.26 for the CSL Elite.

**The car would not move from rest with the wheel turned.** Traction
control measured slip against the body's forward speed; a steered front
wheel rolls along a longer path, which read as wheelspin, so it cut the
engine (the hatch crawled at 1.3 m/s after 4 s at full lock, its engine
at 1127 rpm). It now measures each wheel against its own contact patch's
speed along its heading: at full lock the hatch reaches 7.2 m/s in 4 s,
as at half lock.

**The electric sedan's snap oversteer.** A turn-in at 100 km/h and a lift
slid its rear to 16 degrees (the hatch: 2.6). Lift-off regeneration
added to it but was not the cause (it is about 0.05 g; without it the
slide was still 7.6 degrees, and sharing it between the axles by load,
now done, changed it only to 15.7). The cause was the roll stiffness: 49 %
at the front for 48 % of the weight, so at the limit the lightly loaded
inside rear lost grip first. 5000 N/m moved from the rear bar to the
front, total unchanged, gives 55 % at the front, a few percent above the
weight share as road cars are set: the same manoeuvre now slides 6.9
degrees. Skidpad 0.953 to 0.943 g (published 0.96), understeer gradient
1.83 to 1.88 deg/g. The front-drive hatch keeps its rear-biased roll
stiffness, chosen for lift-off rotation; it does not snap.

Tests (`tests/drive_feel.rs`): every car pulls away above 5 m/s in 4 s at
half and full lock; the sedan's lift-off slide stays under 10 degrees;
the torsion bar passes steady load to 1 % and cuts tick-to-tick roughness
to under 35 % of the rack side's.

## D-089 Directly read pedals followed the steering wheel

With the wheel, pedals and shifter read on the physics thread (D-070,
`use_direct`), throttle, brake and clutch all moved with the steering
wheel. The bridge read every number in the front end's map with gdext's
`try_to::<f64>()`, which accepts only a float Variant. Axis and button
numbers arrive as ints (from `input.cfg`, `axis=5`), so each fell back to
its default of 0: every pedal read axis 0, the steering axis, and every
shifter gate read button 0. Steering itself was right only because it is
axis 0.

Fix: one `number()` helper in the bridge reads a float or an int, used for
the direct-input map and for setup values (which had the same strict read).
`parse_direct_inputs` returns the bindings the bridge would use without
opening a device; `godot/tests/direct_map.gd` feeds it a map built from a
config file in the saved format and checks the axes (0, 1, 5, 6), the
pedal curves and a gate button. Built with the old strict read, the test
fails on throttle, brake, clutch and the gate, as reported.

## D-090 A parked car's wheel shook

The owner reported the wheel shaking with the car stationary. Reproduced
in a closed loop (`a_parked_cars_wheel_stays_still_with_hands_off`): the
CSL Elite preset, a 0.02 kg·m² rim turned only by the force feedback, its
angle read in 16-bit steps over 880 degrees and fed back as steering, the
torque 4 ms late. Hands off, the wheel settled into a steady shake of
about ±0.7 degrees near 8 Hz (rim speed up to 0.28 rad/s, ±0.18 N·m at
the rim) while the tire load stayed near zero.

Cause: D-088 passed the whole rack-side torque through the torsion bar's
second-order filter (natural frequency 7.6 Hz), including rack friction
and reflected inertia. Those two answer the driver's own movement of the
wheel, which the simulation imposes on the column directly; delayed by
the bar's resonance, the friction (a steep damper within 0.15 rad/s)
stopped damping the rim and drove it.

Fix: only the road's load (tire and assist) goes through the bar; friction
and inertia are added after it. Hands off, the parked wheel now only
creeps by one angle step at a time (under 0.05 rad/s; 0.006 seen).

Effect on D-088: the bar still cuts the road load's tick-to-tick
roughness to 36-40 % and its largest one-tick jump from 1.5-2.1 N·m to
0.38 N·m, so kerb kicks stay softened. The felt torque's overall
roughness on a scripted lap is now close to the rack side's (0.91), since
most of it is friction reversing with the scripted driver's quick hand
movements; D-088's figure of under 35 % had counted that friction as
filtered. The torsion-bar test now measures the road load it filters
(under 45 % roughness, largest jump under a third); the steady-corner
check is unchanged.

## D-091 Car models built in Blender from the data files

The owner asked for the cars to be made in Blender. The models are built
by a script, `tools/blender/make_car.py` (run for every non-variant car by
`tools/make_car_models.ps1`), rather than by hand, so that each body is
sized from the same numbers the simulation uses and can be rebuilt when
they change: length, width, body height, ground clearance, wheelbase,
centre-of-gravity position, both tracks and the tire size. The body is a
mirrored cage of cross-sections from nose to tail, subdivided twice, with
the belt line and sill creased; the wheel arches are cut 6 cm clear of the
tire, and the faces the cut leaves form a dark wheel well. Lamps, grille,
intake, plates and sills are thin panels projected onto the body; the
windscreen sits 0.95 m ahead of the seat's eye point (as the interior in
car_view.gd), the overhangs are equal (as the sim's body box). The
shapes are original and generic: a two-box hatch under 4.3 m, a
three-box fastback above. Drawing only; nothing feeds the simulation.

Outputs: `godot/assets/cars/<id>.glb` (body plus one wheel per axle, about
8 400 faces), an editable `art/cars/<id>.blend`, and three preview renders
in `docs/cars/`. The front end loads the model for the car's id, or for
its base car's id with trailing `_suffix` parts removed (the limited-slip
Tern GT uses the Tern GT's), and keeps the generated body (D-055) for a
car without one. The paint is single-sided, as before, so the
driver's-seat view looks out through the shell; the door mirrors are
visible from the seat. Hand edits to a .blend are overwritten when the
script is rerun. Checked by `godot/tests/car_model.gd` and by in-game
screenshots of both cars.

## D-092 Each car drawn so its type reads at a glance

The owner asked for the models to look more like the cars they stand for
and to carry more detail, without being those cars. Each car now has a
style in `tools/blender/make_car.py` that draws on the look of its type,
not of one model, and copies no badge, name, grille or lamp graphic:

- Tern GT, a hot hatch: upright nose with slim headlamps joined by a black
  grille band and a thin red line, a large mesh lower intake with side
  intakes and fog lamps, five-door glasshouse with a black B-pillar and a
  small quarter light ahead of a thick forward-leaning C-pillar, roof
  spoiler with a brake light over a steep tailgate, a wiper, twin round
  tailpipes, five split-spoke silver wheels.
- Halden E4, an electric sport sedan: low rounded nose with no upper
  grille, swept headlamps set into its top, a slim lower intake, a more
  raked screen, one sweep of dark glass from screen to boot, flush door
  handles, a short boot with a black lip spoiler, tail lamps wrapping onto
  the sides, a finned diffuser, dark ten-blade turbine wheels.

Both gain panel gaps (doors, bonnet, tailgate or boot), door handles,
mirrors on arms, black sills, a front and rear plate, wheel nuts, tread
grooves and red brake calipers. The calipers are their own meshes: the
car view puts them under the steering pivot but outside the spinning
wheel, so they steer and do not turn. The side glass is now drawn as
panels on black surrounds, which lets window outlines slope (A- and
C-pillars) instead of following the body's cross-sections. About 12 000
faces per car.

Not changed: the dimensions. The Tern GT's placeholder size (3.95 m long,
2.50 m wheelbase) is shorter than its comparable, and the overhangs stay
equal (D-091); both are simulation parameters, left as they are.


## D-093 The Tern GT is based on the 2011 MINI Cooper S three-door

The owner asked for the hatch to be based on a 2011 MINI Cooper S
three-door, in its physics and its look. The Tern GT keeps its fictional
name and id; its data now follows that car where a figure was found
(sources in docs/VALIDATION_SOURCES.md), and its comparable for
validation changes from the Golf GTI Mk7 to the Cooper S.

From published figures (`estimated`): length 3.729 m, width 1.683 m,
height 1.258 m above a 149 mm ground clearance, wheelbase 2.467 m, tracks
1.453 / 1.461 m, mass from Edmunds' measured 1190 kg plus a driver, Cd
0.36 x 1.99 m2, the six-speed gearbox (3.308 to 0.816, final drive
3.706), 260 N.m from 1700 to 4500 rpm (the overboost figure; full
throttle is what the short validation runs use) and 135 kW at 5500 rpm,
195/55 R16 tires (a new placeholder file derived from the 225/40 R18 by
size), 294 / 259 mm discs, 2.4 turns lock to lock and a 10.7 m turning
circle, from which the ratio is derived as in D-083: 13.40. Scaled from
the old values, keeping ride frequencies and balance (`placeholder`):
springs, dampers, anti-roll bars, inertias, unsprung masses, brake
torques and disc heat capacities. Not found, left as placeholders: weight
distribution (62 % front), centre-of-gravity height (0.53 m), rev limit
(6500 rpm).

No instrumented road test of the 2011 hatch was found online, so its
targets mix sources, each labelled: 60-0 mph 113.5 ft (Edmunds, Cooper S
models on 17 inch wheels), 0-60 mph 6.6 s (the maker's estimate, no
rollout), 228 km/h (maker), skidpad 0.87 g (Edmunds, the 2009 convertible
of the same generation). The validation test now asks each car for one
metric of each kind, so the hatch can use 60-0 and 0-60 without rollout
where the sedan uses Car and Driver's 70-0 and rollout times.

**Torque curves are net output.** The first run had the 0-60 at 7.28 s,
just outside tolerance. The cause was in the engine model, not the data:
the torque curve is the published figure, which is measured at the crank
and so is already net of the engine's own friction, but the model
subtracted the friction terms again at full throttle, about 27 N.m on this
engine at 5000 rpm, 10 % of its output. The engine now gives
`throttle x (curve + friction) - friction`: the curve at full throttle,
the same engine braking as before with the throttle shut. This applies to
every engine car (the electric sedan is untouched). The hatch's 0-60
became 7.17 s (+8.6 %, inside tolerance), its top speed 64.6 m/s. Two
vehicle tests that sat at a threshold moved with it and were made
decisive without changing what they check: the bump-start test drops the
clutch at 3 m/s rather than 4 (sixth then turns the engine well under the
stall speed), and the missed-downshift test does its money shift at
26 m/s rather than 24 (the run-up now ends in third, not second).

Results: skidpad 0.931 g against 0.87 (+7.0 %), 60-0 34.57 m against 34.59
(-0.1 %), 0-60 7.17 s against 6.6 (+8.6 %), top speed 64.65 m/s against
63.33 (+2.1 %), understeer 1.92 deg/g against a 2.5 guess. Nothing was
tuned to the targets. Replays and ghosts recorded with an engine car
before this change will not reproduce: the engine makes more torque.

## D-094 The Tern GT drawn as a small upright three-door

With the Tern GT based on the 2011 MINI Cooper S (D-093), its model is
redrawn to read as that kind of car without copying it: a new "small"
style in `tools/blender/make_car.py`. The body follows the new
dimensions; the shape has an upright face and screen, a level belt
line, tall flat sides with little tumblehome, a flat roof running nearly
to a near-vertical tailgate, and one long door each side. Cues of the
type, all original in their drawing: round headlamps with chrome bezels
and light rings set into a bonnet whose shut line runs under them and
back along the sides, a small chrome-barred grille and a wide mesh intake
in the bumper with round fog lamps, a bonnet scoop, black pillars under a
roof (and mirror caps) in a contrasting white, black arch trims, upright
corner tail lamps with chrome surrounds, a roof spoiler, twin pipes in the
middle, silver split five-spoke wheels on the 16 inch tire, and dark
calipers in place of the hot hatch's red. No badge, name or the maker's
grille or lamp graphic is reproduced.

The overhangs stay equal, as the simulation's body box (D-091); the real
car's are not, so the drawn nose is a little shorter and the tail a
little longer than the real car's. About 13 000 faces.

## D-095 A softer, rounder Tern GT

The owner asked for the Tern GT to look more organic and closer to its
comparable car. The "small" style in `tools/blender/make_car.py` now has:
a rounder cross-section, where the side bulges, tucks in hard underneath
and rolls over a soft shoulder into glass that leans in further; softer
creases at the belt and sill; ends rounded in plan view (the nose and
tail are narrower than the body); a bonnet that falls further to the
nose; and flared arches. The flares are made by pulling the doors and
bumpers in by up to 35 mm away from the wheels, so the widest point is
still the data's overall width. The black arch trims sit on the flares
and are cast in several rows so they follow its curve. The headlamps and
grille are larger, and the tail lamps move inboard for the rounder tail.
The other cars are unchanged. The overhangs stay equal (D-094). About
14 500 faces.

## D-096 Front overhang in the car data

Until now the body box (ground and wall contact) and the drawn body were
centred between the axles, so every car had equal overhangs. The real
small hatch the Tern GT is based on (D-093) has a long nose and a short
tail, and the owner asked for the model to match it. `body.front_overhang_m`
is a new optional figure, front axle to nose. Without it the overhangs
stay equal, so the Halden E4 and older data are unchanged.
`BodyParams::front_overhang` and `box_center_x` give the figure and the
box's place; the contact points (`qts-vehicle`), the wall and cone box
(`qts-sim`), the generated body in Godot and the Blender model all use
them, so what is drawn is what collides.

The Tern GT takes 691 mm, from BMW Group press technical data for the
2011 MINI hatch, seen only as a search-result excerpt (691 / 565 mm; the
sheet itself redirects and was not read). The two add up to the 3723 mm
version's length less the wheelbase; the data keeps the 3729 mm length,
so the rear overhang is 571 mm. Confidence `estimated`. The body box
moves 61 mm forward, which changes only body contact: tire forces,
masses and the validation results are unchanged.

## D-097 Turbofan wheels and corner headlamps on the Tern GT

The owner asked for white turbofan wheels and headlamps out at the
corners, so the car reads less like a modern electric truck and more like
its small-hatch comparable. In `tools/blender/make_car.py` the "small"
style now has:

- A wheel with a near-solid face painted white, slightly dished, with 16
  narrow slots swept round it like a fan, and a domed centre cap. The
  slots use a two-sided material because the left wheels are mirrored.
  The other cars keep their spoked wheels.
- Headlamps moved out to the top corners of the face (from 58 to 76
  percent of the half-width), cast along a direction leaning outward so
  they wrap the corner. The full glowing ring is gone: each lamp is a
  chrome bezel round a silver reflector, with a lit projector lens above
  the middle and the amber indicator low inside the lamp.

No maker's wheel or lamp design is copied. Drawing only; the physics is
unchanged.

## D-098 Grille between the lamps, rally turbofans

Following the owner's notes on D-097:

- The headlamps come back in a little, to 70 percent of the half-width.
- A larger six-sided grille sits between them, its top level with the
  lamps, with a chrome surround and four chrome bars. The bonnet's shut
  line runs across just above it, from lamp to lamp, as on the
  comparable car, where the grille sits below the bonnet between the
  lamps. The shape is a plain hexagon, not a copy of the maker's.
- The wheels are now rally-style turbofans, not the swirled fan of
  D-097: a flat white disc almost to the rim, 28 straight vanes leaning
  the same way round its outer ring, and a flat centre with four exposed
  nuts. The owner asked for no research here, so the design is generic.

Drawing only; the physics is unchanged.

## D-099 Flat aero-disc wheels on the Tern GT

The owner sent a photo of the wheels they wanted: flat white aero discs.
The Tern GT's wheel (the "small" style in `tools/blender/make_car.py`)
now follows that layout: a flat white face almost to the rim, a ring of
40 short radial slots just inside the rim, a ring of 20 small round holes
nearer the middle, and a red domed centre cap with a gold ring. The
photo's lettering and badge are left off, since the car is fictional.
This replaces the slanted-vane wheel of D-098. Drawing only.

## D-100 Aero discs: outer slots only, black centre lock

Following the owner's notes on D-099: the ring of inner holes is gone,
leaving only the 40 slots just inside the rim, and the red cap with its
gold ring is replaced by a black centre lock. The lock is a stepped
round base with a six-sided nut on it, and stands about 4.7 cm proud of
the face, just past the tire's sidewall. Its base uses a two-sided
material, because the lathed faces would otherwise be culled and show the
white disc through. Drawing only.
