<!-- SPDX-License-Identifier: Apache-2.0 -->
# Architecture

This document describes the design of the simulator foundation: what the pieces
are, how data moves between them, how determinism is kept, and what is risky.
It is written before the code and updated when the code deviates from it.
Non-obvious choices are logged with their reasons in `DECISIONS.md`.

The product name lives in exactly two places: `README.md` and
`crates/qts-core/src/constants.rs`. This document says "the sim" on purpose.

## 1. Scope of the foundation

In scope: a deterministic headless physics core, two fictional cars, three
placeholder tracks, a force-feedback pipeline with DirectInput output, a
telemetry logger and importer, a validation harness, and a thin Godot 4 front
end.

Out of scope (interfaces only, or nothing): netcode, matchmaking, ranking,
anti-cheat, graphics polish, Steam integration, real-world calibration.

**Honesty rule.** Every vehicle, tire and surface parameter is a placeholder
until calibrated. No document, UI string or test name may call a model
"realistic" or "accurate" unless a validation test against *measured* target
data passes. The harness always prints its error percentages.

## 2. Workspace layout

```
quickturnsim/
├─ Cargo.toml                 workspace root, shared lints and profile settings
├─ crates/
│  ├─ qts-core/               math, fixed-step driver, snapshot, hash, replay, input frames, net hooks, constants
│  ├─ qts-tire/               Magic Formula baseline, transient, thermal, surface interface
│  ├─ qts-vehicle/            chassis, suspension, steering, brakes, drivetrains, aero
│  ├─ qts-ffb/                torque pipeline, device trait, platform backends
│  ├─ qts-telemetry/          CSV logger, CSV importer
│  ├─ qts-sim/                library: session, tracks, data loading, real-time host; binary: headless CLI
│  ├─ qts-validate/           scenario drivers, metric extraction, target comparison
│  └─ qts-godot/              GDExtension binding (cdylib)
├─ data/
│  ├─ cars/                   one TOML file per car
│  ├─ tires/                  one TOML file per tire
│  ├─ surfaces/               one TOML file per surface
│  ├─ tracks/                 one TOML file per track
│  ├─ ffb/                    pipeline presets
│  ├─ import/                 column-mapping profiles for the telemetry importer
│  └─ validation/             target values and tolerances
├─ godot/                     Godot 4 project (scenes, GDScript for rendering, input and UI only)
├─ docs/                      telemetry format, import guide, Steam note, data format reference
└─ .github/workflows/ci.yml
```

### Crate dependency graph

```
qts-core ◄── qts-tire ◄── qts-vehicle ◄─┐
   ▲                                     │
   ├────── qts-ffb ◄─────────────────────┤
   ├────── qts-telemetry ◄───────────────┤
   │                                  qts-sim (lib + bin) ◄── qts-validate
   │                                     ▲
   └─────────────────────────────────  qts-godot
```

Arrows point from dependent to dependency. There are no cycles and nothing
below `qts-sim` performs file or device I/O except `qts-telemetry` (files) and
`qts-ffb::platform` (devices).

### Deviations from the suggested layout

1. **`qts-core` is the bottom layer, not the top.** It owns the *mechanism*
   (fixed-step driver, snapshot and hash traits, replay container, input
   frames) but not the concrete world. This keeps it free of any dependency on
   vehicle or tire code, so every other crate can use its math and snapshot
   traits.
2. **`qts-sim` is a library plus a thin binary.** The concrete `Session`
   (car + track + timing), the track and surface map, and the data-file loader
   need a home that both the headless binary and the Godot binding can link.
   Making `qts-sim` a library avoids a ninth crate. The binary is a
   command-line wrapper around the library and contains no logic of its own.

## 3. Conventions

- **Units:** SI everywhere in code and data. Angles in radians. Data-file keys
  carry the unit as a suffix (`mass_kg`, `spring_rate_n_per_m`). Conversion to
  mph, km/h, degrees or g happens only at display and report edges.
- **Axes:** ISO 8855. Vehicle frame x forward, y left, z up, right-handed.
  World frame z up. The Godot binding converts to Godot's y-up frame; nothing
  else knows about Godot's axes.
- **Tire sign convention:** slip ratio `κ = (Ω·Re − Vx) / |Vx|`, slip angle
  `tan α = −Vy / |Vx|` in the wheel frame, so positive slip produces positive
  force (the convention used in Pacejka's *Tire and Vehicle Dynamics*).
- **Precision:** `f64` for all simulation state. The binding converts to `f32`
  for rendering.
- **Wheel order:** front-left, front-right, rear-left, rear-right, always as a
  fixed-size array `[T; 4]`.

## 4. Simulation core (`qts-core`)

### 4.1 Fixed step

The sim advances in fixed ticks. The default rate is 1000 Hz and is a session
parameter (`tick_rate_hz`), not a compile-time constant. One tick is

```
state[n+1] = step(state[n], input[n], n)
```

`step` reads no clock, no global, no thread-local and no random source.
A `FixedStepDriver` converts elapsed wall time into a whole number of ticks
plus an interpolation fraction; that driver lives *outside* the pure step and
is only used by hosts (the Godot binding, the real-time host). The headless
binary and all tests call `step` directly with a tick count.

Rendering never drives physics. The host keeps the previous and current render
poses and the front end interpolates between them with the fraction from the
driver.

### 4.2 Integration scheme

Semi-implicit (symplectic) Euler: forces are evaluated from the current state,
velocities are updated, then positions are updated from the new velocities.
At 1000 Hz this is stable for the stiffest element in the model (tire vertical
stiffness on unsprung mass, about 18 Hz, and the tire carcass on wheel
inertia, about 40 Hz). Friction-type couplings that would be stiff under an
explicit scheme (clutch lock-up, brake lock, limited-slip locking) are solved
per tick as "the torque that would synchronise the two sides this tick,
clamped to capacity", which is unconditionally stable.

### 4.3 Snapshot, restore and hash

Every stateful type implements one trait:

```rust
pub trait Snapshot {
    fn save(&self, w: &mut StateWriter);
    fn load(&mut self, r: &mut StateReader) -> Result<(), SnapshotError>;
}
```

`StateWriter` is a plain little-endian byte writer (`f64::to_bits`, fixed
field order). There is no serialization dependency in the snapshot path.

The per-tick **state hash** is 64-bit FNV-1a over the snapshot bytes. Because
the hash and the snapshot share one code path, a field cannot be in one and
missing from the other. Parameters loaded from data files are not part of the
snapshot; they are covered by a separate **content hash** of the loaded data,
stored in replay headers so a replay refuses to run against different data.

### 4.4 Input frames and replay

An `InputFrame` is the complete driver input for one tick, stored as
fixed-point integers (steering, throttle, brake, clutch, handbrake as `i16`,
buttons as a bit field). Hosts quantise device values into a frame; the sim
converts the integers back to `f64` deterministically. Quantising at the
boundary means a recorded replay and a network packet carry exactly what the
sim consumed.

A replay file holds a header (format version, tick rate, car and track ids,
content hash), the initial snapshot, the input frame for every tick, and a
state hash every N ticks. Replaying restores the snapshot, feeds the frames,
and compares hashes; the first mismatching tick is reported.

### 4.5 Networking hooks (interfaces only)

`qts-core::net` defines types and traits and no implementation beyond a local
loopback used by tests:

- `InputFrame` with a tick stamp: what a client would send.
- `SnapshotEnvelope { tick, hash, bytes }`: what a server would send.
- `trait InputSource`: where a session gets its frame for tick `n` (local
  device, replay, or later a network buffer).
- `trait SnapshotSink`: where authoritative snapshots are delivered.
- `trait TickClock`: maps a remote tick estimate to a local tick target.

Rollback is possible with these pieces (restore snapshot, re-simulate frames)
but is not built.

## 5. Determinism strategy

Rules enforced in all crates that run inside `step` (`qts-core`, `qts-tire`,
`qts-vehicle`, and the `session` and `track` modules of `qts-sim`):

1. No wall-clock time. `std::time` is not imported in sim modules.
2. No randomness. Road roughness is a pure function of position (integer
   lattice hash plus interpolation), so it has no state and no seed drift.
3. No `HashMap` or `HashSet`. Collections are arrays and `Vec` with a fixed
   order. A clippy `disallowed-types` rule enforces this.
4. No threads inside a step. A session is stepped by one thread.
5. All transcendental math goes through `qts_core::math::fm` (`sin`, `cos`,
   `atan2`, `exp`, `tanh`, `powf`). Today these forward to `std`. This is the
   single switch point for item 2 of the cross-platform plan below.
6. No fast-math flags, no `mul_add` (Rust does not contract floating-point
   expressions on its own, so FMA only appears if written).

**What is guaranteed:** the same binary on the same machine architecture
produces the same hash sequence for the same snapshot and inputs. The
determinism, replay and snapshot tests check exactly this.

**What is not guaranteed and is not solved:** bit-identical results across
operating systems, CPU architectures, compiler versions or optimisation
levels. `std` transcendental functions call the platform math library, which
differs between Windows, Linux and macOS, and debug and release builds may
order operations differently.

**Plan for cross-platform, in order of cost:**

1. *Now:* replays record the build target triple and crate version; loading a
   replay from a different build warns and reports the first diverging tick
   instead of silently drifting.
2. *Next:* switch `fm` to a pure-Rust software math library (the `libm`
   crate). IEEE-754 add, subtract, multiply, divide and square root are exactly
   specified, so with software transcendentals the remaining divergence sources
   are compiler reordering and FMA contraction, both avoidable. This needs a
   cross-platform CI job that compares hash sequences before it can be claimed.
3. *For ranked play:* the server is authoritative regardless. Clients send
   input frames; the server's snapshot hash is the truth and clients correct
   to server snapshots. Cross-platform bit-exactness then becomes a bandwidth
   optimisation, not a correctness requirement.

## 6. Vehicle model (`qts-vehicle`)

**Bodies and degrees of freedom.** One rigid sprung body with six degrees of
freedom (position, orientation quaternion, linear and angular velocity). Four
unsprung masses, each with one vertical degree of freedom along the suspension
axis and one spin degree of freedom. Total: 14 degrees of freedom plus
drivetrain and tire internal states.

**Suspension.** Per corner: spring and damper acting through a motion ratio,
bump and rebound damping rates, bump stops, and an anti-roll bar per axle
coupling left and right travel. Basic geometry is data-driven: static camber
and toe, camber gain with travel, caster, kingpin inclination, scrub radius,
mechanical trail, roll-centre height per axle (applied as a jacking force so
geometric and elastic load transfer are separated), and anti-dive and
anti-squat fractions.

**Tire contact.** The tire is a vertical spring and damper between the
unsprung mass and the ground, so tire load is a result of the dynamics, not an
algebraic weight-transfer formula. Ground height, normal and surface come from
the track query.

**Steering.** Steering-wheel angle maps through a ratio and an Ackermann
fraction to the two front wheel angles. The **raw steering torque** is the sum
over both front wheels of the moments about the kingpin axis (tire aligning
moment, lateral force times mechanical trail, longitudinal force times scrub
radius, and the load-dependent jacking terms from caster and kingpin
inclination), referred to the steering column through the ratio. It is
computed every tick and is an output only: the steering wheel position is an
input from the device.

**Brakes.** Pedal to line pressure to per-wheel torque with a front/rear bias,
a handbrake on the rear axle, lock handling by the synchronising-torque
method, and an optional data-driven ABS (per-wheel slip threshold modulation).

**Drivetrains.** A `Drivetrain` trait takes driver inputs and wheel speeds and
returns per-wheel drive torques and reflected inertias. Two implementations:

- *Front-drive manual:* engine (torque curve table, inertia, friction, idle,
  rev limiter), friction clutch, manual gearbox with shift time, final drive,
  and a differential that is either open or a clutch-type limited slip
  (preload plus torque-sensitive ramp), selected in the car file.
- *Dual-motor electric all-wheel drive:* two motors with torque/speed
  envelopes, single-speed reductions, open differentials, a `TorqueVectoring`
  hook that may adjust the front/rear split and request per-wheel brake
  torque (the default implementation passes through), and regenerative
  braking blended with the friction brakes: brake demand is met by motor
  regeneration up to motor, battery and low-speed limits and the remainder
  goes to the friction brakes. A simple battery tracks energy for the energy
  checks.

**Aerodynamics.** Drag area, lift area, and front/rear lift distribution, with
air density as a parameter.

## 7. Tire model (`qts-tire`)

Four layers, each usable and testable alone:

1. **Steady-state baseline.** A documented subset of the Magic Formula
   (coefficient names follow MF 5.2 / PAC2002 where a coefficient is
   implemented): pure longitudinal and lateral force with load sensitivity and
   camber, combined slip through cosine weighting functions, and aligning
   moment through pneumatic trail and residual torque. Tests check the
   formula against independently computed reference values and against its
   analytic properties (peak equals `D`, initial slope equals `BCD`).
2. **Transient.** Single-contact-point relaxation: longitudinal and lateral
   carcass deflection states with relaxation lengths, so force builds over
   rolled distance. The formulation remains valid at standstill, with the
   usual low-speed damping term.
3. **Thermal and pressure.** Surface, core and carcass temperature nodes plus
   inflation gas. Heat comes from sliding power and rolling hysteresis and
   leaves by conduction to the road, convection to air, and conduction between
   nodes. Grip scales with a temperature window; pressure follows the gas
   temperature and scales cornering stiffness, peak friction, vertical
   stiffness and relaxation length.
4. **Surface interface.** A `SurfaceParams` set per surface: peak-friction
   scale, stiffness scale, rolling resistance, roughness, and loose-surface
   terms (post-peak plateau and a slip-dependent ploughing force). Tarmac is
   the supported case. Gravel is a working first pass and is marked untuned in
   its data file and in `STATUS.md`.

No tire or surface number is compiled in. Every parameter is read from
`data/`.

## 8. Data files

TOML, parsed with `serde`. Every physical parameter is a table carrying its
value and provenance:

```toml
mass_kg = { value = 1250.0, source = "class-typical placeholder", confidence = "placeholder" }
```

`confidence` is one of `placeholder`, `estimated`, `measured`. The loader
rejects a parameter without `source` and `confidence`, and can print a
provenance report listing every non-measured parameter. Both cars are
fictional; names and numbers refer to no real vehicle.

## 9. Force feedback (`qts-ffb`)

```
raw column torque (N·m, physics rate)
   → gain and N·m-to-device scaling
   → low-pass filter
   → slew-rate limiter
   → + damping, friction, inertia (from measured wheel velocity)
   → + optional curb and road-detail channels (band-passed front-axle signals)
   → soft clip (smooth saturation, never a hard clamp)
   → device
```

The pipeline is a plain struct stepped once per physics tick. It keeps both
the pre-filter and post-filter signal and hands them to telemetry, so clipping
and latency can be measured offline. It is **not** part of the deterministic
sim state: it depends on the physical wheel's motion, so it is an output-side
consumer and is excluded from the state hash.

Device output sits behind a trait:

```rust
pub trait FfbDevice {
    fn info(&self) -> &DeviceInfo;
    fn set_torque(&mut self, normalized: f32) -> Result<(), FfbError>;
    fn read_steering(&mut self) -> Result<Option<f32>, FfbError>;
    fn stop(&mut self) -> Result<(), FfbError>;
}
```

Backends: `null` (records output; used in tests and when no wheel is present)
and `dinput` (Windows DirectInput constant-force effect, compiled only on
Windows). A Linux backend can be added as another module without touching the
pipeline. Presets are data files; the entry-level direct-drive-style wheelbase
preset shipped with the foundation is labelled as initial and untuned.

## 10. Hosts and data flow

```
            ┌──────────── Godot process ─────────────────────────────────────┐
 devices →  │ GDScript: read input, map and calibrate, draw, HUD, overlays   │
            │        │ quantised input            ▲ interpolated poses, HUD   │
            │        ▼                            │                           │
            │ qts-godot (binding): type conversion and axis mapping only     │
            └────────┼────────────────────────────┼──────────────────────────┘
                     ▼                            │
            ┌── qts-sim::host (real-time host thread) ──────────────────────┐
            │ FixedStepDriver → Session::step(input, tick) at 1000 Hz       │
            │        │ raw steering torque          │ telemetry sample      │
            │        ▼                              ▼                       │
            │   FfbPipeline ──► FfbDevice      TelemetryWriter (CSV)        │
            │        ▲ wheel position/velocity                              │
            └───────────────────────────────────────────────────────────────┘

 headless:  qts-sim binary → Session::step in a plain loop (no host thread,
            no clock) → replay file, telemetry CSV, hash log
```

The real-time host owns a sim thread so physics and force feedback run at the
physics rate regardless of the render frame rate. The host uses the wall clock
only to decide *how many* ticks to run; it never passes time into `step`.
Godot holds no simulation state: it sends inputs and reads poses.

## 11. Telemetry and validation

**Telemetry.** A CSV file with `#`-prefixed metadata lines, one header row and
one row per sample at a configurable decimation of the physics rate. Channels
cover chassis motion, driver inputs, per-wheel slip, load, temperatures,
pressure and forces, and the force-feedback signals before and after the
pipeline. The format is specified in `docs/TELEMETRY_FORMAT.md`.

**Import.** `qts-telemetry::import` reads CSV exports from data loggers using a
column-mapping profile (a TOML file naming the time, speed, acceleration,
yaw-rate and steering columns and their units). It produces a normalised run
and summary metrics, and can emit a validation-target snippet tagged
`confidence = "measured"` with the source file name. The guide is
`docs/TELEMETRY_IMPORT.md`.

**Validation.** `qts-validate` runs scripted manoeuvres with closed-loop driver
models and compares the results with `data/validation/*.toml`:

| Metric | Manoeuvre |
| --- | --- |
| Maximum lateral acceleration | Constant-radius circle, slowly increasing speed |
| Braking distance from 60 mph | Straight-line stop from 26.82 m/s |
| 0 to 60 mph time | Standing start, driver model controls launch |
| Top speed | Full-throttle run until acceleration falls below a threshold |
| Understeer gradient | Constant-radius circle, steering angle against lateral acceleration |

Each metric has a target, a tolerance, a source and a confidence. The harness
prints simulated value, target, error percentage and pass or fail, and exits
non-zero beyond tolerance. Targets are placeholders until real data is
supplied, and the report says so on every line.

**Other tests.** Hash-sequence determinism, record-and-replay, snapshot and
restore mid-run, Magic Formula reference values, energy checks (a passive car
never gains energy; electric drive energy balances battery energy), and a
10-minute headless soak with scripted inputs that fails on any non-finite or
out-of-bounds state.

## 12. Front end (`godot/` and `qts-godot`)

The binding exposes one class with methods to list cars and tracks, start a
session, push inputs, fetch interpolated transforms and HUD values, fetch
track geometry for drawing, configure and select a force-feedback device, and
start or stop telemetry and replay recording. GDScript builds placeholder
meshes from the track geometry, handles keyboard, gamepad and wheel input with
a calibration and mapping screen, and draws the HUD and the debug overlay.
There is no physics, timing or lap logic in GDScript.

## 13. Portability

- Windows-specific code exists only in `qts-ffb::platform::dinput`, behind
  `cfg(windows)`. Everything else builds on Linux; CI builds and tests on both.
- Paths are built with `std::path`; data files use forward slashes and LF.
- The binding uses GDExtension, which Godot supports on Windows, Linux and
  Steam Deck.
- Steam: the planned route is GodotSteam as a separate GDExtension in the
  front end; the note is in `docs/STEAM.md`. Nothing is integrated now.

## 14. Milestones

1. Workspace, licenses, `qts-core` (math, driver, snapshot, hash, replay,
   input frames, net hooks) with tests.
2. `qts-tire`: baseline, transient, thermal, surfaces, data files, tests.
3. `qts-vehicle`: chassis, suspension, steering, brakes, aero.
4. Drivetrains and the two car data files.
5. `qts-sim`: tracks, session, data loading, headless binary, replay.
6. `qts-telemetry` and `qts-ffb` pipeline.
7. `qts-validate` and the full test suite.
8. DirectInput backend and the real-time host.
9. `qts-godot` binding and the Godot project.
10. README, CONTRIBUTING, CI, docs, `STATUS.md`.

Each milestone ends with the test suite passing and a commit.

## 15. Risks

| # | Risk | Effect | Mitigation |
| --- | --- | --- | --- |
| 1 | All parameters are placeholders | The cars drive plausibly at best; numbers mean nothing yet | Provenance on every parameter; honesty rule; import path for real data |
| 2 | Cross-platform float determinism | Replays and hashes differ between OS or build | Documented limit; single math switch point; server-authoritative plan |
| 3 | Force feedback cannot be felt by automated tests | Preset may feel wrong, oscillate or be reversed on real hardware | Pipeline unit tests (clip, slew, filter); pre/post logging; conservative default gain; sign-check step in the setup screen; marked untuned |
| 4 | DirectInput behaviour varies by driver | Device may refuse exclusive access or the effect type | Null backend fallback; errors surfaced in UI; device code isolated |
| 5 | Stiff couplings at 1000 Hz | Clutch, brake or differential chatter; instability at standstill | Synchronising-torque solves; low-speed tire damping; soak test |
| 6 | Reduced Magic Formula subset | Cannot load a published tire file directly | Coefficient names kept compatible; subset documented; extension is additive |
| 7 | Flat ground only | No elevation, banking or kerb geometry beyond a height profile | Track query returns height and normal already; heightfields are a data change |
| 8 | Godot and binding version drift | Extension fails to load on a newer editor | Pin versions in docs; binding is thin; CI builds it |
| 9 | Driver models in validation | A metric can reflect the test driver, not the car | Driver models are simple, documented and reported with each metric |
| 10 | Real-time thread scheduling on Windows | Tick jitter adds force-feedback latency | Host measures and logs tick jitter; catch-up is bounded |
| 11 | Loose-surface model is a first pass | Gravel behaviour is qualitative only | Marked untuned in data, UI and status |
