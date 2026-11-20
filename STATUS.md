<!-- SPDX-License-Identifier: Apache-2.0 -->
# Status

Current state of the project. "Verified" below means it was run on the development machine (Windows 11, Rust 1.100 with
the GNU toolchain, Godot 4.7.2) and observed to work. Anything not verified
says so.

## Summary

| Area | State |
| --- | --- |
| Workspace builds with one command on Windows | Verified (`tools\build.ps1`, GNU toolchain) |
| Test suite | 223 tests pass in release |
| Headless sim runs and replays exactly | Verified |
| All cars drivable on all four tracks | Verified with scripted drivers and the front-end smoke test |
| Front end renders, HUD and overlay work | Verified by screenshots |
| Force-feedback output to a wheel | Verified on a Fanatec CSL Elite with `qts-sim ffb-test`: device opens, torque moves the wheel, sign is correct |
| Driving with a wheel in the front end | Driven by the project owner on a Fanatec CSL Elite; force feedback works. Preset still untuned |
| Keyboard, gamepad and wheel input in the front end | Wheel driven by the project owner; keyboard and gamepad not checked by a person |
| CI | **Runs green** on every push to main: format, lint, release build, the whole test suite and the validation report, on Windows (MSVC) and Linux |
| MSVC build on Windows | Verified on CI (the development machine uses the GNU toolchain) |
| Linux | Built and tested on CI; the front end and force feedback are not tried on Linux |
| Release package | `toolspackage.ps1` makes a release export (129 MB, 42 MB zipped); `--self-test` drives it from a copy outside the repository |
| Realism | **Not claimed.** Every parameter is a placeholder |

The original definition of done is met: the project owner has driven it
with a wheel and force feedback, and all tests pass on CI. What a player
sees in the demo is described in `DEMO.md`.

## What works

### Simulation core
- Fixed-step simulation at a configurable rate (1000 Hz default) as a pure
  function of state, input and tick.
- Snapshot and restore of the complete state, with a 64-bit hash taken over
  the same bytes.
- Input recording and exact replay, with hash checkpoints and detection of
  the first diverging tick. Replays refuse to run against changed data.
- Determinism on one build: identical hash sequences for identical inputs,
  tested for every car on every track. Transcendental math is computed in
  software, so a replay recorded in the game reproduces in the command-line
  tool and in playback (D-066). Across operating systems: likely, untested.
- Ten-minute soak with erratic inputs for four car and track combinations:
  no non-finite values and no runaway states.

### Vehicle
- Sprung body and four unsprung masses solved together each tick. Load
  transfer under braking and cornering matches the textbook figures within
  5 to 6 percent in tests.
- Springs, dampers, anti-roll bars, travel stops, camber and toe, roll-centre
  and anti-pitch geometry, Ackermann steering, steering compliance.
- Raw steering torque from tire forces about the kingpin axes.
- Brakes with bias, handbrake and a simple anti-lock controller.
- Traction control on all three cars (D-054): one torque fraction for the
  whole car, with a switch in the menu (D-063).
- Drag and lift.
- Engine, clutch, six-speed manual gearbox, open or limited-slip
  differential, with optional automatic clutch and gear selection.
- Dual-motor electric all-wheel drive with power limits, a battery energy
  account, lift-off regeneration and regeneration blended with the friction
  brakes.
- Energy checks: an undamped, frictionless car conserves energy without
  drift; a passive car never gains energy; electric drive energy balances
  against the battery.

### Tires
- Magic Formula subset (pure and combined slip, aligning moment, camber,
  load sensitivity, pressure terms), matching an independent implementation
  to ten significant figures.
- Relaxation-length transient that also holds the car at standstill.
- Surface, core, carcass and gas temperatures, with a grip window and
  pressure following gas temperature. Heat is conserved in the closed test.
- Per-surface parameters. Tarmac, gravel, verge and kerb exist.

### Force feedback
- Pipeline with gain, low-pass, slew limit, damping, friction, inertia,
  road-detail and kerb channels and a soft clip, all unit tested.
- Pre-filter and post-filter signals logged to telemetry.
- DirectInput backend: compiles, enumerates devices, structure sizes match
  the DirectInput ABI, errors are reported. A test torque on a Fanatec CSL
  Elite moved the wheel in the commanded direction, and the project owner
  has driven with it.
- Tuning tools (D-069): live meter (F4) of output against request, knee
  and full scale with the recent peak; minimum force for weak bases; a
  one-lap log of every pipeline stage (F5) and `qts-sim ffb-report`.
- Wheel, pedals and shifter can be read on the physics thread every tick
  (D-070): 0.09 ms from read to physics against 15.9 ms of hand-over at
  60 fps through Godot. Shared DirectInput access, so the same base can be
  pushed and read.

### Telemetry, import, validation
- Telemetry CSV with 108 documented channels; a test keeps the document and
  the code in step.
- Logger CSV importer with column-mapping profiles and run summaries,
  tested against synthetic runs with known answers.
- Validation harness with five metrics per car. Current numbers against the
  placeholder targets:

| Metric | Tern GT (hatch) | error | Halden E4 (sedan) | error | tolerance |
| --- | --- | --- | --- | --- | --- |
| Skidpad lateral | 0.910 g vs 0.930 | -2.2% | 0.956 g vs 0.950 | +0.7% | 10% |
| 60-0 mph distance | 36.1 m vs 35.0 | +3.0% | 34.5 m vs 33.5 | +3.0% | 10% |
| 0-60 mph | 6.57 s vs 6.30 | +4.3% | 3.00 s vs 2.95 | +1.6% | 10% |
| Top speed | 69.7 m/s vs 69.4 | +0.4% | 71.8 m/s vs 72.2 | -0.6% | 6% |
| Understeer gradient | 2.09 deg/g vs 2.50 | -16.3% | 1.68 deg/g vs 2.00 | -16.0% | 40% |

These targets are approximate class-typical figures, not measurements.
Passing them shows the harness works and the placeholder cars are in a
plausible range. It is not evidence of realism.

### Front end
- Title screen with the circuit flown over behind it; car and track
  selection with assists, wheel and preset; the last choices are
  remembered. Pause menu (resume, restart, end, quit) and end-of-session
  results with every counted lap, the best one and the replay (D-068).
- Settings screen (display, sound, view) and force-feedback adjustments
  saved per preset (D-052).
- Ghost of the best lap per car and track, saved between sessions (D-051).
  A live time difference to that lap is shown while driving (D-063).
- A run counts only if it passes every checkpoint in order with a plausible
  distance and time, and on Ridgeline never has all four wheels beyond the
  road and kerbs (D-064, D-072); a HUD warning shows the moment a lap is
  lost to track limits. Invalid saved ghosts are ignored.
- Garage (D-071): pressures, camber, toe, springs, dampers, bars, brake
  bias, limited-slip differential and gearing within club-car ranges,
  saved per car and track and stored in every replay and best lap.
- H-pattern shifter and analog clutch (D-073): each gate mapped on the
  Controls screen; without the clutch a gear grinds and stays out unless
  the revs match; the engine stalls, starts on the starter with the clutch
  down, and bump-starts. Sequential shifting remains.
- Clutch pedal with free travel, a bite zone (position and width set in
  the garage) and a free floor, drawn on the Controls screen from the
  physics (D-074). Peak engine speed and over-revs past the limiter are
  shown on the HUD; optional engine damage from over-revving, down to
  failure. Clutch shudder and gear grind reach the wheel as light FFB cues.
- Steering system in the physics (D-075): speed-sensitive power assist
  with an on-centre zone, rack friction and inertia, gyroscopic moments of
  the front wheels and weight moments through caster and kingpin
  inclination; `qts-sim steering-compare` and
  `docs/img/steering_torque.svg` show column torque against speed before
  and after.
- Brake disc temperature per wheel with fade, speed-dependent cooling and
  garage brake ducts; tire wear from sliding energy, lowering grip and
  thinning the tread (D-076). Both on the F1 overlay and in telemetry.
- Standalone release package via `tools/package.ps1` (D-053): 129 MB on
  disk, 42 MB zipped. Verified to start and drive (`--self-test`) from a
  copy outside the repository with Rust and Godot off the path; not yet
  tried on a second machine.
- Placeholder world built from track data: ground, surface regions, road
  strip, cones that tip when hit, timing gates, painted markers.
- Generated low-poly car bodies (two shapes) with glass, lamps, mirrors and
  spoked wheels (D-055); chase and driver's-seat cameras.
- Track map with three modes (N), painted edge lines, distance boards
  before corners, worn-in rubber on an approximate fast line, tire marks
  left by the car while sliding, and trackside trees (D-056, D-059, D-061).
- Raised kerbs on every Ridgeline corner, solid for the tires, whose ramps
  and tapered ends tilt the contact so they push sideways (D-065, D-072); a
  generated sky with cloud, cascaded sun shadows, varied terrain colour
  and mown runoff.
- Solid trees and barriers (D-060, D-062): the car is stopped by them,
  with a thud and a force-feedback kick. A simple flat impact model with
  no damage. The kick has not been felt on a wheel.
- HUD: speed, gear, engine speed or battery, run timer, last and best time,
  cones. Debug overlay: per-wheel slip, slip angle, load, three
  temperatures, pressure and surface; steering torque; force-feedback
  signal and clipping; physics rate and scheduling jitter.
- Controls screen: assign and calibrate axes and buttons, force-feedback
  settings with live sliders.
- Every session is recorded; replays can be watched with trackside,
  chase, orbit and driver's-seat cameras, paused and scrubbed (D-067).
- Telemetry recording from the keyboard; F3 saves the replay so far.

### Content
- Tern GT, a fictional front-drive hot hatch, with a limited-slip variant.
- Halden E4, a fictional dual-motor electric sedan.
- Lot Autocross (412 m cone course, 69 cones: slalom, sweeper, offset, hairpin), Proving Ground (timed
  quarter-mile strip, 30 m skidpad, kerb strip, gravel patch), Quarry Road
  Stage (1001 m, gravel changing to tarmac at 758 m), Ridgeline Circuit
  (fictional 2.85 km closed circuit, 38 m of elevation, banked corners).

## What is stubbed or missing

- **Networking**: types and traits only (`qts_core::net`). No netcode,
  matchmaking, ranking or anti-cheat, by design.
- **Torque vectoring**: the hook exists; the only strategy is a fixed
  front/rear split.
- **Elevation**: one path per track can carry an elevation and banking
  profile (D-049); only Ridgeline Circuit uses it. Each wheel sees the
  ground as a tilted plane, so sharp edges and kerbs as geometry are still
  missing. The terrain away from the road is a smooth interpolation, not a
  modelled landscape, and the drawn terrain sits 8 cm below the real one.
- **Linux force-feedback output**: none. The trait and the null device are
  there; a backend is not.
- **Cones** are detected and counted, not simulated as objects.
- **Impacts**: trees and barriers stop the car with a rigid impulse on the
  whole car. No damage, no deformation, nothing through the suspension.
- **Stability control**: none. Traction control is not per axle.
- **Engine damage**: over-revving only; no overheating, oil or
  mechanical wear. The damage and failure speeds are placeholders.
- **Steering rack as a dynamic part**: the rack follows the driver's
  angle exactly; friction and inertia are felt but do not move it.
- **Chassis flex, driveline compliance, gearbox efficiency, brake pad
  temperature and fluid boiling, wet surfaces, wind**: not modelled. Brake
  fade uses the disc temperature as one lumped mass.
- **Cross-platform determinism**: not solved; the plan is in
  `ARCHITECTURE.md` section 5.
- **Steam**: nothing integrated; see `docs/STEAM.md`.
- **Audio**: synthesised placeholder loops for engine, motor, tires, road and wind (D-045). Not tuned by ear; no recorded samples.

## What is untuned

Everything numeric. In particular:

- **All car parameters**: class-typical placeholders. About 190 parameters per
  car with its tires and the surfaces, none measured
  (`qts-sim provenance --car <id>`).
- **Tire coefficients**: textbook-typical shapes, not fitted to any tire.
- **Gravel and verge surfaces**: a first-pass loose-surface model, marked
  `tuned = false`, flagged in the HUD while driving on it.
- **Thermal model**: order-of-magnitude heat capacities and conductances.
- **Force-feedback presets**, including the Fanatec CSL Elite one: written
  from the pipeline design and the published peak torque. Gain, filtering,
  the device update rate and the **output sign** are unverified.
- **Anti-lock controller, automatic clutch, automatic shift points**.
- **Keyboard steering and gamepad response curves** in the front end.
- **Validation driver models**: simple by design; the 0-60 figure in
  particular depends on the launch routine.

## Known issues and caveats

- Selecting a wheel pauses the simulation for a few hundred milliseconds
  while DirectInput enumerates devices.
- Input read through Godot arrives once per frame and is ramped across the
  frame interval, adding up to one frame of latency. Controls assigned to
  be read on the physics thread avoid this; controls mapped before D-070
  need assigning once more for it.
- Replays and ghost laps recorded before the setup and track-limits
  changes still load; replays made before track limits report that they
  no longer reproduce from the start.
- Debug and release builds are different builds: a replay recorded by one is
  not expected to match on the other, and the replay command warns about it.
- The `windows` crate is pinned to 0.58 because newer versions do not link
  on the GNU toolchain without extra tools (D-026).
- The binding library is about 25 MB because it contains the full
  generated Godot API.
- Commits were made with a repository-local git identity; change it with
  `git config user.name` and `user.email` if needed.

## Next five things to do

1. **Retune the CSL Elite preset from the lap logs.** Three F5 logs of
   Ridgeline, then `qts-sim ffb-report`: set the gain so fast corners stop
   clipping and the fall in torque past the front tires' peak reaches the
   wheel. The steering model of D-075 sends less torque than the one
   the preset gain was set against, so expect the gain to go up.
2. **Measure the input delay on the wheel.** With steering assigned both
   ways, the F1 overlay and `recordings/input_delay.log` give the real lag
   of the per-frame path; confirm the physics-thread path on hardware.
3. **Bring in real data.** Import skidpad, launch and braking runs
   (`docs/TELEMETRY_IMPORT.md`), replace the placeholder targets with
   measured ones, and calibrate the tires and cars.
4. **Cross-platform determinism check.** Run the same inputs on Windows and
   Linux in CI and compare hash sequences.
5. **Pedals and shifter on hardware.** Set the bite point by feel on real
   pedals, try the H-pattern, stall and over-rev model on a real shifter,
   and set the shudder and grind cue strengths on the wheel.

After those: a Linux force-feedback backend and the first torque-vectoring
strategy.
