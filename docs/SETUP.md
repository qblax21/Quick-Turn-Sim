<!-- SPDX-License-Identifier: CC-BY-4.0 -->
# Car setups

The garage changes a car from its data file within ranges a club-level car
of its class could be set to. A setup file lists only what was changed;
everything else stays as in `data/cars`. The code is
`crates/qts-sim/src/setup.rs`; the tests that each adjustment reaches the
physics are in `crates/qts-sim/tests/setup.rs`.

All ranges are **placeholders** chosen from general knowledge of road and
club-racing cars, not from measurement, like the car values they start
from.

| Adjustment | Range | Why this range |
| --- | --- | --- |
| Cold tire pressure, per axle | 150 to 300 kPa gauge (22 to 44 psi) | Road tires of this size run about 200 to 260 kPa; below 150 the bead and sidewall are at risk, above 300 is beyond any sensible road or track use |
| Camber, per axle | -4.0 to +0.5 deg | Strut cars with camber plates reach about -3.5; multi-link rears about -3; positive camber is rarely wanted |
| Toe per wheel, per axle | -0.5 to +0.5 deg | Ordinary alignment adjustment; beyond this tire wear makes it pointless |
| Spring rate, per axle | 0.6 to 2.5 times the car's | Softer road springs to stiff coilovers for a street-legal car |
| Damper bump and rebound, per axle | 0.5 to 2 times the car's | The span of an adjustable damper's clicks |
| Anti-roll bar, per axle | 0 (disconnected) to 2.5 times the car's, at least 20 N/mm | Removed bar to an aftermarket adjustable bar |
| Brake bias to the front | 50 % to 85 % (or 5 % above the car's own, if higher) | Bias valve or balance bar adjustment; front-drive road cars run 75 to 80 % |
| Brake duct opening, per axle | 0 to 100 % | From no duct to a fully open cooling duct; a fully open duct doubles the speed-dependent disc cooling (placeholder) |
| Differential type | open, clutch pack, locked | An aftermarket clutch-pack unit or a competition spool can be fitted to an engine car (D-084) |
| Differential preload | 0 to 200 N.m | Spring preload of clutch-pack units; acts only with a clutch pack |
| Differential power and coast ramp angles | 30 to 90 deg | Ramp angles clutch-type units are sold with; lower locks harder, 90 gives none (one-way on coast) |
| Clutch bite point | 30 % to 65 % of pedal travel | Cable or master-cylinder pushrod adjustment; road cars bite from just above to a little below mid-travel |
| Clutch bite zone width | 10 % to 40 % of pedal travel | A sharp competition clutch to a soft road clutch |
| Final drive | 0.85 to 1.15 times the car's | Alternative final-drive sets |
| Each gear ratio | 0.9 to 1.1 times the car's, each gear taller than the one below | Alternative gear sets |

The electric sedan has no gearbox or differential adjustment (one motor per axle, each with a fixed open differential); its motor
reduction gears are fixed.

A setup travels with every recording. Replays store it in their header
(format version 2) and are rebuilt with it, so they reproduce exactly;
ghost laps store it so the HUD and results can say which setup the best
lap was driven with. Replays and ghosts from before setups read as the car
as delivered.
