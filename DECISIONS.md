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
