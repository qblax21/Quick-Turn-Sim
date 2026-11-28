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

(240 characters. "A download under 20 MB": the full game zips to 19.6 MB and the demo to 18.9 MB [measured: D-132, D-121; Steam's own compression will differ somewhat].)

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
- The full game is a 19.6 MB download and 49.7 MB installed; the demo is
  18.9 MB and 49 MB **[measured: D-132 and D-121, `tools\package.ps1
  -Ship` and `-Demo`; the download is the zip, and Steam's own compression
  will differ somewhat]**.
- Harrow on High at 1080p uses about 0.7 GB of RAM and 0.8 GB of video
  memory **[measured on an RTX 3060 only: D-128]**.

## System requirements (draft)

**An estimate (D-136) until it is run on real low-end hardware.** Every
figure was measured on one PC (Ryzen 5 5600X, RTX 3060, Windows 11); the
GPU lines are scaled from it by FP32 throughput, which is crude.

| | Minimum: Very Low (OpenGL), 720p, about 45 fps | Recommended: Medium or High (Vulkan), 1080p, 60 fps | Status |
|---|---|---|---|
| OS | Windows 10 64-bit | Windows 10/11 64-bit | **[UNMEASURED]** only Windows 11 has been run |
| CPU | 4 threads (Core i3-6100 / Ryzen 3 1200 class) | 6 threads (Ryzen 5 2600 / Core i5-8400 class) | **[ESTIMATE]** physics is 45 us of each 1 ms tick on one 5600X core; drawing 2.2 ms (Vulkan) to 2.9 ms (OpenGL) a frame on another [measured, D-136] |
| RAM | 4 GB | 8 GB | Peaks: Very Low 0.32 GB, Low 0.66 GB, High 0.67 GB [measured, D-128, D-136] |
| GPU | OpenGL 3.3, 1 GB (Intel UHD 620 class) | Vulkan 1.3, 2 GB (GTX 1050 Ti / RX 570 class) | **[ESTIMATE]** RTX 3060 GPU time per frame: Very Low 0.68-0.76 ms, Low 0.47-0.67 ms, High 2.2 ms at 1600x900 [measured, D-134, D-136], scaled about 30 and 6 times. Video memory by the driver: Very Low 82 MB, Medium 0.56 GB, High 0.82 GB [measured] |
| Storage | 100 MB | 100 MB | The full game installs in 49.7 MB [measured, D-132]; the line leaves room for settings, replays and updates |
| Input | Keyboard, gamepad or a DirectInput wheel | A force-feedback wheel | Wheels **[UNMEASURED]** on hardware |

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
- **Smoke:** the replay viewer draws tyre smoke as live driving does
  (D-133), so the trailer and screenshots show it.
- **No music or sound:** the trailer is silent.
