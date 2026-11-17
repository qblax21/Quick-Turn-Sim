<!-- SPDX-License-Identifier: Apache-2.0 -->
# Status

State of the foundation at the end of the first build. "Verified" below
means it was run on the development machine (Windows 11, Rust 1.100 with
the GNU toolchain, Godot 4.7.2) and observed to work. Anything not verified
says so.

## Summary

| Area | State |
| --- | --- |
| Workspace builds with one command on Windows | Verified (`tools\build.ps1`, GNU toolchain) |
| Test suite | 163 tests pass in release |
| Headless sim runs and replays exactly | Verified |
| Both cars drivable on all three tracks | Verified with scripted drivers and the front-end smoke test |
| Front end renders, HUD and overlay work | Verified by screenshots |
| Force-feedback output to a wheel | Verified on a Fanatec CSL Elite with `qts-sim ffb-test`: device opens, torque moves the wheel, sign is correct |
| Driving with a wheel in the front end | Driven by the project owner on a Fanatec CSL Elite; force feedback works. Preset still untuned |
| Keyboard, gamepad and wheel input in the front end | **Not verified by a person.** Code runs; no device input was exercised |
| CI | Written, **never run** (the repository has not been pushed) |
| MSVC build on Windows | **Not verified** (no Windows SDK on the development machine) |
| Linux | Type-checks (`cargo check --target x86_64-unknown-linux-gnu`); **not built or tested** |
| Realism | **Not claimed.** Every parameter is a placeholder |

Two items in the definition of done are therefore open: driving with a
wheel with force feedback working needs a person at a wheel, and "all tests
pass" is true locally but unconfirmed on CI.

## What works

### Simulation core
- Fixed-step simulation at a configurable rate (1000 Hz default) as a pure
  function of state, input and tick.
- Snapshot and restore of the complete state, with a 64-bit hash taken over
  the same bytes.
- Input recording and exact replay, with hash checkpoints and detection of
  the first diverging tick. Replays refuse to run against changed data.
- Determinism on one build: identical hash sequences for identical inputs,
  tested for every car on every track.
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
  whole car, always on.
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
  Elite moved the wheel in the commanded direction. Driving through the
  front end with it is still untested.

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
- Menu, car and track selection, assists, wheel and preset selection; the
  last choices are remembered.
- Settings screen (display, sound, view) and force-feedback adjustments
  saved per preset (D-052).
- Ghost of the best lap per car and track, saved between sessions (D-051).
  The recorder is tested; the ghost has not yet been seen on screen by
  anyone, because that needs a completed lap by a person.
- Standalone package via `tools/package.ps1` (D-053). Only the fallback
  development package has been built: export templates are not installed.
- Placeholder world built from track data: ground, surface regions, road
  strip, cones that tip when hit, timing gates, painted markers.
- Generated low-poly car bodies (two shapes) with glass, lamps, mirrors and
  spoked wheels (D-055); chase and driver's-seat cameras.
- Track map with three modes (N), painted edge lines, distance boards
  before corners, worn-in rubber on an approximate fast line, tire marks
  left by the car while sliding, and trackside trees (D-056, D-059, D-061).
- Solid trees and barriers (D-060, D-062): the car is stopped by them,
  with a thud and a force-feedback kick. A simple flat impact model with
  no damage. The kick has not been felt on a wheel.
- HUD: speed, gear, engine speed or battery, run timer, last and best time,
  cones. Debug overlay: per-wheel slip, slip angle, load, three
  temperatures, pressure and surface; steering torque; force-feedback
  signal and clipping; physics rate and scheduling jitter.
- Controls screen: assign and calibrate axes and buttons, force-feedback
  settings with live sliders.
- Telemetry and replay recording from the keyboard.

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
- **Replay playback in the front end**: replays can be recorded and are
  verified headless, but cannot be watched.
- **H-pattern shifter in the front end**: the core accepts a direct gear
  selection; the front end only sends sequential shifts.
- **Cones** are detected and counted, not simulated as objects.
- **Impacts**: trees and barriers stop the car with a rigid impulse on the
  whole car. No damage, no deformation, nothing through the suspension.
- **Power steering**: a fixed assist fraction per car, not a speed-sensitive
  model.
- **Stability control**: none. Traction control exists but cannot be
  switched off and is not per axle.
- **Engine stall and starter**: implicit (see D-020).
- **Wheel gyroscopic effects, chassis flex, driveline compliance, gearbox
  efficiency, brake temperature and fade, tire wear, wet surfaces, wind**:
  not modelled.
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
  frame interval, adding up to one frame of latency. The direct-steering
  option avoids this for the wheel but is untested.
- Debug and release builds are different builds: a replay recorded by one is
  not expected to match on the other, and the replay command warns about it.
- The `windows` crate is pinned to 0.58 because newer versions do not link
  on the GNU toolchain without extra tools (D-026).
- The binding library is about 25 MB because it contains the full
  generated Godot API.
- Commits were made with a repository-local git identity; change it with
  `git config user.name` and `user.email` if needed.

## Next five things to do

1. **Put a wheel on it.** With the CSL Elite attached: run
   `qts-sim ffb-test` to confirm the device opens and find the output sign,
   then drive, watch the overlay for clipping and scheduling jitter, and
   tune the preset. This closes the open item in the definition of done and
   is the first real test of the project's second differentiator.
2. **Bring in real data.** Import skidpad, launch and braking runs
   (`docs/TELEMETRY_IMPORT.md`), replace the placeholder targets with
   measured ones, and let the validation failures drive calibration of the
   tires and cars.
3. **Push and get CI green** on MSVC and Linux, then add a job that compares
   hash sequences between the two with the software math library switched
   in, to find out how far cross-platform determinism actually is.
4. **Deepen the tire model where the data points**: fit Magic Formula
   coefficients to real tire data, extend the coefficient set as needed, and
   tune or replace the loose-surface pass for gravel.
5. **Kerbs and bumps as geometry**: elevation and banking exist now; give
   the gravel stage crests and camber, and add kerbs and bumps so the
   suspension and force-feedback detail channels get real inputs.

After those: a Linux force-feedback backend, replay playback in the front
end, and the first torque-vectoring strategy.
