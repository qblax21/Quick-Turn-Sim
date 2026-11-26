<!-- SPDX-License-Identifier: CC-BY-4.0 -->
# Steam store page: draft text and the Coming Soon kit (D-129)

A draft, not published anywhere. Every claim is marked:

- **[measured]**: measured in this repository; the decision record says
  where.
- **[UNMEASURED]**: an estimate or a plan. It must be measured, or taken
  out, before the page goes live.

The physics is not calibrated against real cars (README, STATUS.md), so the
text never says "realistic" or "accurate".

## Short description (store capsule, at most 300 characters)

> A physics-first driving sim you buy once. Grip laps and judged drifting
> on fictional cars and tracks, a 1000 Hz simulation, force feedback from
> the tyre forces, and a download under 20 MB. No subscription, no paid
> DLC: every update is free.

(240 characters. "A download under 20 MB" is the demo's 18.9 MB [measured]; the full game's is [UNMEASURED].)

## About this game

**Buy it once. Every improvement after that is free.** Quick Turn Sim has
no subscription and no paid add-ons, and there never will be. If you want
to help fund development, there is an optional donation link; it unlocks
nothing.

**Physics first.**
- The simulation runs at a fixed 1000 steps a second on its own thread
  **[measured: D-001, physics tick 44 to 46 us mean on an RTX 3060 machine,
  D-126]**.
- Tyres use a Magic Formula model with temperature and pressure.
- Every replay re-simulates exactly from your inputs **[measured: replay
  checks report EXACT]**.

**Drift, judged.**
- Harrow Drift Park has painted clipping points.
- A judge scores line, angle and style corner by corner.
- A coach tells you where the points went and, when you spin, the likely
  reason.
- Run tandem behind a computer-driven lead car.

**Grip laps.**
- Ridgeline Circuit, a ghost of your best lap, and a corner-by-corner coach.
- Lap telemetry you can export and compare.

**Force feedback from the physics.**
- The steering torque comes from the tyre forces at the front axle, with a
  clipping meter and one-key gain.
- **[UNMEASURED]** The wheel presets have not been tried on a wheel yet
  (README: "Force feedback: before you drive with a wheel").

**Small and light.**
- The demo is an 18.9 MB download and 49 MB installed **[measured: D-121,
  `tools\package.ps1 -Demo`]**.
- **[UNMEASURED]** The full game's size is not measured; the demo is two
  cars and two tracks.
- Harrow on High at 1080p uses about 0.7 GB of RAM and 0.8 GB of video
  memory **[measured on an RTX 3060 only: D-128]**.

## System requirements (draft)

| | Minimum | Status |
|---|---|---|
| OS | Windows 10 64-bit | **[UNMEASURED]** only Windows 11 has been run |
| CPU | 4 cores (Ryzen 3 3100 / Core i3-10100 class) | **[UNMEASURED]** estimate (D-117); one core runs the physics at about 4 % of a Ryzen 5 5600X core |
| RAM | 4 GB | **[UNMEASURED]** for 4 GB. The game peaks at 0.67 GB on High on the RTX 3060 [measured, D-128]; the old 8 GB line was a guess |
| GPU | Vulkan 1.3, GTX 1050 Ti / RX 570 class, 2 GB | **[UNMEASURED]** scaled from the RTX 3060 (D-117, D-125); video memory on High fits a 2 GB card by the budget [measured on the 3060: 0.82 GB] |
| Storage | 150 MB | **[UNMEASURED]** for the full game; the demo installs in 49 MB [measured] |
| Input | Keyboard, gamepad or a DirectInput wheel | Wheels **[UNMEASURED]** on hardware |

## Media

Made with `tools\steam_kit.ps1`, from in-game footage only (replays of the
computer driver drawn by the replay viewer).

| Asset | Size | What |
|---|---|---|
| `screenshots\screenshot_1_ridgeline.png` | 1920x1080 | Ridgeline hot lap, chase camera |
| `screenshots\screenshot_2_ridgeline.png` | 1920x1080 | Ridgeline from above |
| `screenshots\screenshot_3_harrow.png` | 1920x1080 | Harrow tandem, low behind the follower |
| `screenshots\screenshot_4_harrow.png` | 1920x1080 | Harrow tandem, trackside |
| `screenshots\screenshot_5_harrow.png` | 1920x1080 | Harrow tandem from above |
| `trailer_1080p60.mp4` | 1920x1080, 60 fps, 38 s, H.264 | Ridgeline hot lap (18 s), then Harrow in tandem (20 s) |

**Not made yet**, because they need artwork beyond in-game footage:
- the capsule images: header 920x430, small 462x174, main 1232x706 and
  vertical 748x896;
- the library assets;
- a logo.

**Footage notes:**
- **The tandem is two computer-driven laps:** a lead started 30 m ahead,
  as the game's tandem mode starts it, and a follower from the line. They
  run as separate sessions with no contact, as in the game.
- **No smoke:** the replay viewer does not draw tyre smoke yet (D-117), so
  the trailer has none, although live driving does.
- **No music or sound:** the trailer is silent.
