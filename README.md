<!-- SPDX-License-Identifier: Apache-2.0 -->
# Quick Turn Sim

Quick Turn Sim (QTS) is an open-source, physics-first racing simulator. This
repository is its **foundation**: a deterministic headless physics core in
Rust, a force-feedback pipeline, telemetry, a validation harness, and a thin
Godot 4 front end with two fictional cars and three placeholder tracks.

> **Read this first.** Nothing here is calibrated. Every vehicle, tire and
> surface parameter is a placeholder with a recorded source and confidence
> level, the cars are fictional, and the validation targets are approximate
> class-typical numbers, not measurements. The project does not claim any
> model is realistic. `STATUS.md` says what works, what is stubbed and what
> is untuned.

## What is in the box

- **Physics core** (`crates/`): fixed 1000 Hz step, snapshot and restore,
  exact replay, a state hash every tick. Runs without a window.
- **Vehicle model**: sprung body, four unsprung masses, suspension with
  basic geometry, steering, brakes with anti-lock, aerodynamics, and two
  drivetrain modules (engine with manual gearbox, dual-motor electric with
  blended regeneration).
- **Tire model**: Magic Formula baseline, relaxation-length transient,
  three-node thermal model with pressure, per-surface parameters. Tarmac is
  the supported surface; gravel is a first pass and is marked untuned.
- **Force feedback**: raw steering torque from the physics through a
  configurable pipeline to a DirectInput device on Windows.
- **Telemetry**: documented CSV, plus an importer for data-logger CSV.
- **Validation harness**: scripted manoeuvres compared with target files,
  reporting the error for every metric.
- **Front end** (`godot/`): rendering, input and UI only.

What a player sees in the demo: `DEMO.md`.
Design and reasoning: `ARCHITECTURE.md`, `DECISIONS.md`.

## Requirements

| What | Version | Notes |
| --- | --- | --- |
| Rust | stable, 1.85 or later | Install from <https://rustup.rs>. |
| Godot | 4.4 or later (standard build, not .NET) | Only for the front end. Developed against 4.7.2. |
| Windows | 10 or 11 | Linux builds and passes type-checking; no force-feedback output there yet. |

On Windows, either Rust toolchain works:

- **MSVC** (rustup's default): needs the Visual Studio C++ build tools with a
  Windows SDK.
- **GNU**: needs nothing else. Use it if you do not have the build tools:
  `rustup default stable-x86_64-pc-windows-gnu`

## Build (Windows, one command)

From the repository root:

```powershell
powershell -ExecutionPolicy Bypass -File tools\build.ps1
```

This builds every crate in release mode and copies the binding library to
`godot\bin\`. Add `-Test` to run the whole test suite afterwards.

On Linux: `tools/build.sh` (add `--test` for the tests).

## Package a standalone folder (Windows)

```powershell
powershell -ExecutionPolicy Bypass -File tools\package.ps1
```

This writes `out\<name>\` with the executable, the game pack, the
simulation library and the data files; it runs on a machine with neither
Rust nor the Godot project. With Godot's export templates installed
(Editor > Manage Export Templates) it is a release executable. Without
them the script makes a development package that uses a copy of the Godot
editor binary as its runtime: it plays the same but is larger and is not
what should be shipped.

For the smaller runtime (D-121), build the slimmed release template once
(or download the `godot-template` artifact from a CI run) and pass it:

```powershell
powershell -ExecutionPolicy Bypass -File tools\build_template.ps1
```

```powershell
powershell -ExecutionPolicy Bypass -File tools\package.ps1 -Demo -CheckBudget -Template out\godot-template\godot_template_release.exe
```

`build_template.ps1` needs Python with SCons and Visual Studio with the C++
tools and a Windows SDK.

`Run render bench.bat` in the packaged folder drives a lap of Harrow at
Low and High and writes `render_bench_<date>.txt` with frame times, RAM and
video memory. The memory budget for Harrow on High is in
`tools\memory_budget.txt`; check a build against it with

```powershell
powershell -ExecutionPolicy Bypass -File tools\memory_check.ps1 -Exe out\QuickTurnSimDemo\QuickTurnSimDemo.exe
```

(add `-Software` to render on Chrome's SwiftShader, as CI does; D-128).

Frame times are checked against `tools\bench_baseline.txt` (no frame over
50 ms after 5 s, 1% low no more than 15 % slower; D-130). After a change
that touches rendering, bench it on a real GPU and commit the run:

```powershell
powershell -ExecutionPolicy Bypass -File tools\bench_check.ps1 -Run -Update
```

The store build of the full game is a 19.6 MB zip and installs in
49.7 MB (D-132; most of that is the Godot runtime):

```powershell
powershell -ExecutionPolicy Bypass -File tools\package.ps1 -Ship -Template out\godot-template\godot_template_release.exe
```

The Steam Coming Soon kit (five 1920x1080 screenshots and a 1080p60
trailer from in-game replays, D-129) comes from one command; it needs Godot
and ffmpeg. The draft store text is in `docs/STORE_PAGE.md`.

```powershell
powershell -ExecutionPolicy Bypass -File tools\steam_kit.ps1
```

To check a packaged folder on another machine, run it once with
`QuickTurnSim.exe --self-test`: it drives the first car on the first track
for three seconds, prints the speed reached and exits with 0 if the car
moved.

## Run

### With the front end

1. Open Godot, choose **Import**, and select the `godot` folder.
2. Press **Play** (F5).
3. Pick a car and a track, then **Drive**.

Keyboard and a gamepad work without setup. For a wheel, open **Controls and
wheel setup** first: assign and calibrate the axes, choose the wheel under
**Wheel** in the menu, and read the force-feedback note below.

| While driving | Key |
| --- | --- |
| Accelerate, brake | W / S or the arrow keys |
| Steer | A / D |
| Handbrake | Space |
| Shift up, down | E / Q |
| Clutch | Shift |
| Reset to the start (also repairs an over-revved engine) | R |
| Pause menu | Esc |
| Camera | C |
| Track map: whole track, zoomed, heading up, off | N |
| Ghost of the best lap | G |
| Mute | M |
| Pause | P |
| Tire and force-feedback overlay | F1 |
| Record telemetry | F2 |
| Save the replay so far (every session is recorded) | F3 |
| Force-feedback tuning meter | F4 |
| While the meter shows: gain down, up / minimum force down, up | [ ] / ; ' |
| Log the force feedback for the next full lap | F5 |
| Frame time and physics step overlay | F6 |
| Coach: last lap corner by corner against the best | F7 |
| Drift judging: last lap corner by corner (drift tracks) | F9 |
| Starter (engine stalled: clutch down first) | I |
| Photo mode: free camera, depth of field, high-res stills | F8 |
| Hide everything on screen | H |

Recordings go to the `recordings` folder in Godot's user data directory for
the project. With "Save telemetry for every lap" on in Settings, every lap
is written to its own CSV under `recordings/laps`; `qts-sim lap-compare
A.csv B.csv --svg compare.svg` lines two of them up by distance and shows
where the time went (D-081).

### Tuning a wheel preset

F4 shows the force-feedback meter: the bar is what the wheel is sent, the
white line what was asked for before the soft clip, the orange lines the
largest request of the last two seconds, the dashed lines the soft-clip
knee and the solid lines the wheel's full force. Red means clipping: the
wheel cannot give more, and a change in tire grip there is not felt. The
CLIP lamp at the right end lights while clipping and holds for half a
second; under the bar is the share of this lap and the last spent
clipping, which every per-lap telemetry file also records
(`ffb_clipped_percent`, D-104).

Strength is in real units (D-079): gain is the torque at your wheel per
N·m at the car's steering column, the same on every base, and gain 1 is the
car's real torque. Pick your wheel base on the Controls screen to set its
peak torque (the CSL Elite's 6 N·m is unverified; any figure can be typed
over). `qts-sim ffb-gain` prints the gain that puts hard cornering at the
soft-clip knee of each base.

F5 arms a log of the next full lap: every stage of the force-feedback
signal at 1000 Hz, the steering torque from the physics and the front tire
state, about 20 MB of CSV per lap. Summarise one with

```
qts-sim ffb-report path/to/ffb_log.csv
```

which lists where the lap clipped and how the torque falls as the front
tires pass their grip limit, and suggests a gain. **Minimum force** (F4 with
; and ', or the Controls screen) lifts small forces above the friction of a
weak base; leave it at zero unless small forces disappear.

### Car setups

**Garage...** on the car and track screen adjusts the car within ranges a
club car of its class could run and saves the result by name for that car
and track. The setup is stored in every replay (which is rebuilt with it)
and every best lap. "What it does" shows the ride frequencies, damping
ratios, roll split and differential locking the setup gives; **Export
sheet** writes it all to a text sheet that **Load sheet...** reads back.
Ranges and their reasoning: [docs/SETUP.md](docs/SETUP.md).
For the Kestrel RC, **Safe drift baseline** returns to a drift setup the
computer driver has checked clean on Harrow. When a drift ends in a spin,
F9 names the likely cause (full lock, a sudden throttle, or the rear sliding
past the limit) and any rear grip the setup gives away against that
baseline (D-127).

### H-pattern shifter

On the Controls screen set **Gear change** to "H-pattern shifter" (it
switches itself when you assign a gate), then **Assign** each gate,
Shifter gate 1 to 6 and R, by putting the lever in it. "Lever now" shows
which gate the game reads; move through every gate and check it follows.
With **Automatic clutch** off (car screen) a gear only goes in with the
clutch pedal down, so the clutch must be assigned and calibrated
(released 0 %, floor 100 %). With it on, the lever alone changes gear.
While driving, "lever 3: clutch down to engage" under the gear means the
lever is in a gate the gearbox refused (D-110).

### Wheel and pedals read on the physics thread

On the Controls screen, **Assign** each control and move it: the wheel,
pedals, shifter and buttons are then read straight from the device by the
physics thread, once per tick, instead of once per drawn frame (the box
"Read wheel, pedals and shifter on the physics thread" turns this off).
Controls mapped before this change need assigning again once. Keyboard
and gamepad still work for anything not mapped.

Measured on the development machine (RTX 3060, CSL Elite attached),
from handing the input over to the physics having all of it:

| Path | 60 fps, vsync on | about 430 fps, vsync off |
| --- | --- | --- |
| Per frame through Godot (before) | 15.9 ms mean, 16.9 ms max | 1.5 ms mean, 3.7 ms max |
| Read on the physics thread (now) | 0.09 ms mean, 0.2 ms max | 0.09 ms mean, 0.4 ms max |

The per-frame path also waits for Godot to poll the device, once per
frame, which these figures do not include. With a control mapped both
ways, F1 shows the measured lag of Godot's steering behind the device's,
which does include it, and every session appends its figures to
`recordings/input_delay.log`. `qts-sim input-read` lists every axis and
button the attached controllers report and times the reads.

### Force feedback: before you drive with a wheel

The DirectInput output and the wheel presets have **not been tried on
hardware**. Treat the first run as a test:

1. Start with the gain low (the presets do).
2. Drive slowly and turn. The wheel should resist and return to centre when
   you let go.
3. If it pulls further into the turn instead, tick **Invert force direction**
   in the setup screen.

You can also test a wheel without the front end. This moves the wheel
gently, so keep your hands clear:

```powershell
target\release\qts-sim.exe ffb-devices
target\release\qts-sim.exe ffb-test --device <id from the list>
```

### Headless

```powershell
target\release\qts-sim.exe list
target\release\qts-sim.exe run --car hatch_tern_gt --track gravel_stage --driver path --seconds 90 --replay run.rpl --telemetry run.csv
target\release\qts-sim.exe replay run.rpl
```

`replay` re-simulates the recorded inputs and compares state hashes. On the
build that made the recording it must report `EXACT`. `qts-sim help` lists
every command.

## Tests and validation

```powershell
cargo test --workspace
```

The suite covers the tire model against independently computed Magic
Formula values, weight transfer and energy checks on the vehicle,
determinism, replay, snapshot and restore, a ten-minute soak per car, the
force-feedback pipeline, the data files, and the validation harness.

To see the validation numbers:

```powershell
target\release\qts-validate.exe
```

It prints simulated value, target, error and tolerance for skidpad lateral
acceleration, 60-0 mph braking distance, 0-60 mph time, top speed and
understeer gradient, and exits non-zero if any is out of tolerance. The
shipped targets are placeholders and the report says so.

The front end has a headless smoke test:

```powershell
godot --headless --path godot --script res://tests/smoke.gd
```

## Calibrating against your own data

1. Export a run from your logger as CSV.
2. `qts-sim import your_run.csv --profile generic --targets`
3. Review the summary and paste the generated targets into
   `data/validation/<car>.toml`.

Details and how to write a column-mapping profile for your logger:
`docs/TELEMETRY_IMPORT.md`.

## Repository layout

```
crates/      Rust workspace (qts-core, qts-tire, qts-vehicle, qts-ffb,
             qts-telemetry, qts-sim, qts-validate, qts-godot)
data/        Cars, tires, surfaces, tracks, FFB presets, import profiles,
             validation targets. Never compiled into a crate.
godot/       Godot 4 project: rendering, input, UI.
docs/        Telemetry format, import guide, data file reference, Steam note.
tools/       Build scripts and the Magic Formula reference script.
```

## Determinism

The same build, given the same starting snapshot and the same inputs,
produces the same state hash on every tick. That is tested. Bit-identical
results across operating systems, CPU types or compiler versions are **not**
guaranteed and are not solved; `ARCHITECTURE.md` section 5 gives the plan.

## Price and support

Quick Turn Sim is bought once, and every later improvement is free. There is
no subscription and there are no paid add-ons. If you'd like to help fund
development, there is an optional donation page:
[Support development](https://example.com/quick-turn-sim/support)
(placeholder address). A donation unlocks nothing. The same link sits in the
corner of the title screen, with no reminders or pop-ups. The address is set
in one place, `DONATE_URL` in `crates/qts-core/src/constants.rs`; an empty
string hides the link in the game.

## Licence

- Code: Apache-2.0, see `LICENSE`.
- Original assets and data files: CC BY 4.0, see `LICENSE-ASSETS`.

The cars, tracks and names in this repository are original and fictional.
Hardware product names appear only to identify compatible devices.

## Renaming the project

The product name is written in two places: this README and
`crates/qts-core/src/constants.rs`. The front end and every report read it
from the constants file, and no file format contains it.
