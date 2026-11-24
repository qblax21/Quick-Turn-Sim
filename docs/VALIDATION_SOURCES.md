<!-- SPDX-License-Identifier: CC-BY-4.0 -->
# Validation sources

The two cars in the game are fictional, and stay so: no real name, brand
or likeness appears in the game or in `data/cars`. To validate them against
something real, each is compared with the closest real car for which
instrumented road-test figures are published (D-077). The real names
appear only here and in `data/validation`, at the project owner's request.

The figures are measurements of those real cars, not of ours, so every
target is labelled `estimated`. A pass says the fictional car performs like
a real car of its kind on that one number; it does not say its parameters
are right.

## How the figures compare with the harness

- **Car and Driver** reports 0-60 mph with a one-foot rollout subtracted
  (its published test method; the article pages do not restate it). The
  harness has a matching metric, `accel_0_60_rollout`, which starts the
  clock once the car has moved one foot.
- **Car and Driver** reports braking from 70 mph, not 60. The harness has a
  matching metric, `braking_distance_70_0`, driven exactly like the 60-0 one
  (coast to the speed, full pedal in one step). No conversion between the
  two is used.
- **Skidpad**: Car and Driver uses a 300-ft (91.4 m) diameter circle where
  it says so; the harness circle on the test track has a 30 m radius
  (60 m diameter). The smaller circle is driven slower, so the two figures
  are not exactly the same measure; the difference is not corrected.
- **Top speed**: US test cars are often electronically limited. Where they
  are, a manufacturer's figure is used and labelled as such.
- **Edmunds** reports braking from 60 mph and 0-60 mph without a rollout;
  the harness metrics `braking_distance_60_0` and `accel_0_60` match. A
  maker's 0-60 claim is taken as timed without rollout.
- **Understeer gradient**: no source publishes one. Both targets stay
  `placeholder`.

## Tern GT and the MINI Cooper S (2011, R56)

Since D-093 the Tern GT is based on the owner's choice, the 2011 MINI
Cooper S three-door hatch (R56 after its 2010 update): 1.6 litre turbo
(N18), 135 kW, six-speed manual, front-wheel drive. Before D-093 it was
compared with the seventh-generation Volkswagen Golf GTI; those figures
remain in the history sections below.

No instrumented road test of the 2011 hatch itself was found online
(Car and Driver, Motor Trend and Edmunds archives were searched; Car and
Driver's online tests of the hatch start with the 2014 model, a different
generation). The targets therefore come from Edmunds tests of R56 Cooper S
cars and from the maker, as labelled.

| | Tern GT (data) | Cooper S |
| --- | --- | --- |
| Layout | front transverse engine, front drive, 6-speed manual | same |
| Engine | 260 N.m from 1700 to 4500 rpm, 135 kW at 5500 rpm | 240 N.m from 1600 to 5000 rpm, 260 N.m on overboost (1700 to 4500 rpm), 135 kW at 5500 rpm |
| Gearbox | 3.308, 2.130, 1.483, 1.139, 0.949, 0.816; reverse 3.231; final drive 3.706 | same |
| Differential | open (the `hatch_tern_gt_lsd` variant has a limited-slip) | open; limited-slip optional |
| Mass | 1265 kg with driver | 2623 lb (1190 kg) measured, 2007 car; 1205 to 1215 kg quoted at the kerb |
| Size | 3729 x 1683 mm, 2467 mm wheelbase, tracks 1453 / 1461 mm, overhangs 691 / 571 mm (since D-096) | same; overhangs 691 / 565 mm on the 3723 mm car |
| Tires | 195/55 R16 summer placeholder | 195/55 R16 standard; 17 inch optional |
| Aero | 0.72 m2 drag area | Cd 0.36, 1.99 m2 |
| Steering | 2.4 turns lock to lock, ratio 13.40 derived (D-083 method) | 2.4 turns; 10.7 m turning circle |

Sources of the car data:

| Figures | Source |
| --- | --- |
| Length 3729 mm, width 1683, height 1407, ground clearance 149, wheelbase 2467, tracks 1453 / 1461, kerb 1215 kg, Cd 0.36 x 1.99 m2, 260 N.m at 1700 to 4500 rpm, 135 kW at 5500 rpm, 2.4 turns lock to lock, 195/55 R16 87V on 6.5J x 16, discs 294 / 259 mm, 228 km/h, 0-100 km/h 7 s | [Carfolio, 2010 MINI Cooper S R56](https://www.carfolio.com/mini-cooper-s-216579), read 2026-11-22 (maker's figures as compiled there) |
| Gear ratios, final drive 3.706, reverse 3.231, wheelbase, 10.7 m turning circle, length 3714 mm before the update | BMW Group press technical data for the MINI Cooper S, January 2010 and March 2012, seen as search-result excerpts of press.bmwgroup.com attachments; the attachment links now redirect to the newsroom home page, so the sheets were not read first-hand |
| Overhangs 691 mm front, 565 mm rear | BMW Group press technical data for the 2011 MINI hatch (press.bmwgroup.com attachment T0097274EN_GB), seen as a search-result excerpt on 2026-11-22; the link redirects to the newsroom home page and automobile-catalog.com refused the browser, so it was not read first-hand. The sheet does not say which variant; the two add up to 1256 mm, the 3723 mm length (CarExpert) less the wheelbase. The Tern GT keeps the 691 mm nose and takes the remainder at the rear |
| 240 N.m, 192 lb-ft (260 N.m) on overboost, 181 hp | [Edmunds, 2011 MINI Cooper review](https://www.edmunds.com/mini/cooper/2011/review/), read 2026-11-22 |
| 2623 lb as tested, optional limited-slip differential, 0-60 in 6.5 s (2007 Cooper S, 172 hp) | [Edmunds, 2007 MINI Cooper S long-term road test](https://www.edmunds.com/mini/cooper/2007/long-term-road-test/), read 2026-11-22 |

Targets:

| Metric | Value | Source |
| --- | --- | --- |
| 60-0 mph | 113.5 ft (34.59 m), the middle of 112 to 115 ft | [Edmunds, 2011 MINI Cooper review](https://www.edmunds.com/mini/cooper/2011/review/): "various Mini Cooper S models with 17-inch wheels" |
| 0-60 mph | 6.6 s, no rollout | the maker's estimate for the manual, quoted in the same Edmunds review; Edmunds measured 6.5 s on its 2007 car |
| Top speed | 228 km/h (63.33 m/s) | maker's figure, as listed by Carfolio |
| Skidpad | 0.87 g | [Edmunds, 2009 BMW 128i Convertible vs. 2009 MINI Cooper S Convertible](https://www.edmunds.com/car-reviews/track-tests/track-tested-2009-bmw-128i-convertible-vs-2009-mini-cooper-s-convertible.html) (seen in search results, not read first-hand): the convertible of the same generation; no hatch figure was found |

The understeer target (2.5 deg/g) is a guess; no source was found.

Not found: weight distribution (62 % front is a placeholder), rev limit
(6500 rpm placeholder), the hatch's own skidpad figure, and the
construction of the optional limited-slip differential.

## Kestrel RC and the Mazda RX-7 (FC3S, 1986 to 1992) (D-111)

The Kestrel RC is fictional. Its base is the second-generation Mazda
RX-7 coupe (FC3S, 13B twin-rotor), in the turbo form that drift builds
usually start from: the 1987 RX-7 Turbo II. On top of that it is built
as a modified drift car at the owner's request: about 1.5 times the
factory power, coilovers, lowering, an angle kit and a locked
differential. No validation targets: the car is no longer the stock one.

**Stock factory power: 133 kW (180 PS, 178 bhp) at 6500 rpm, and 247 N.m
at 3500 rpm**, from [Carfolio, 1987 Mazda RX-7 Turbo II](https://www.carfolio.com/mazda-rx-7-turbo-ii-615868),
read first-hand 2026-11-22. One and a half times that is 200 kW, the
Kestrel RC's peak at 6500 rpm. Other sources give 182 hp for the US car
and 200 hp for the 1989 to 1991 update (auto-data.net, search results);
the build is based on the 1987 figure.

| Figure | Value | Source | Status |
| --- | --- | --- | --- |
| Kerb weight | 1250 kg | Carfolio (above) | read first-hand |
| Wheelbase, tracks | 2430 mm; 1450 / 1440 mm | Carfolio | read first-hand |
| Length, width, height | 4290, 1690, 1265 mm | Carfolio | read first-hand |
| Drag coefficient | 0.29 | Carfolio | read first-hand; frontal area (1.75 m2) not found, **unverified** |
| Tires | 205/55 VR16 on 7J x 16 | Carfolio | read first-hand; the tire file is the project's placeholder compound scaled to that size |
| Top gear, final drive | 0.76, 4.10 | Carfolio | read first-hand |
| Gears 1 to 5 | 3.483, 2.015, 1.391, 1.000, 0.762 | Lapmeta and a rotary club forum archive, seen in search results | **unverified** (top gear agrees with Carfolio) |
| Reverse | 3.288 | rx7.net.nz, the later FD3S's sheet (read first-hand) | **unverified** for this car |
| Weight on the front axle | 50.5 % | the maker's launch figure, quoted in search results; Car Throttle questions the "50:50" claim | **unverified** |
| Steering, 2.9 turns lock to lock | 522 degrees each way | rx7.net.nz, the later FD3S's sheet | **unverified** for this car |
| Brakes | vented discs all round, ABS | Carfolio | read first-hand; four-piston fronts from search results, **unverified**; torques are placeholders |
| Rev limit | 7000 rpm | commonly given, not found in a primary source | **unverified** |

The build, all **estimated and unverified**:

| Change | Value | Basis |
| --- | --- | --- |
| Power | 1.5 times the stock torque at every speed: 370 N.m peak, 200 kW at 6500 rpm | the owner's brief; the shape of the stock curve between the two published points is estimated |
| Coilovers | 8 kgf/mm front, 6 kgf/mm rear (78.5 and 58.8 N/mm); ride 2.5 and 1.9 Hz | common street and drift rates for this chassis |
| Dampers | 0.35 of critical in bump, 0.6 in rebound | placeholder |
| Lowering | 30 mm | placeholder |
| Angle kit | 60 degrees of lock (63 inner, 58 outer); Ackermann 0.2 of ideal; caster 7 degrees | FC kits from FDF (Mantis, "up to 70 degrees depending on wheel size and offset", adjustable Ackermann) and FAT ("up to 71 degrees") seen in search results; 60 is taken as a decent rather than maximum kit |
| Steering ratio | 8.70 overall | 60 degrees on the same 522 degrees of wheel travel: kits gain lock with shorter steering arms |
| Differential | locked (welded or spool) | the owner's brief |
| Clutch | 600 N.m | uprated for the torque; placeholder |
| Anti-lock and traction control | off | drift builds disconnect ABS for the handbrake; placeholder |

## Clutch-pack limited-slip differential (D-107)

Used by `hatch_tern_gt_lsd` and whenever a clutch pack is fitted in the
garage. The comparable car offered a limited-slip differential as an
option (Edmunds, above); its construction was not found. Drexler builds
ramp-and-clutch-pack units for this car (BMW MINI R52 to R57, manual) to
order, "custom-built to your specifications with numerous ramp angles"
([Hack Engineering, Drexler limited slip differential (MINI)](https://www.hackengineering.co.uk/product/drexler-limited-slip-differential-mini/),
read 2026-11-22; no figures given). The values come from Drexler's
published instructions for another of its units, and from Cusco's front
unit for a front-drive hatch.

| Parameter | Value | Source | Status |
| --- | --- | --- | --- |
| Power / coast ramp | 40 / 50 deg | Drexler, *Assembly and disassembly instruction, Formula Student Adjustable Limited Slip Differential 2017* (autotech.com/pdf/183884, read first-hand 2026-11-22): "all differentials are assembled in the basic position 40 / 50"; options 45 / 60 and 30 / 45 | estimated: a Drexler unit's basic setting, not the MINI unit's (built to order) |
| Locking per N.m of input | 0.51 / tan(ramp): 0.61 at 40 deg, 0.43 at 50 | same document: "theoretical lock-up torques" 88, 60, 51, 42 and 29 percent at 30, 40, 45, 50 and 60 degrees, which is 0.50 to 0.51 / tan(ramp) at every angle | estimated: matched through plate_friction = 0.1275 with 8 faces and radius ratio 1 |
| Friction faces, plate friction, radius ratio (each) | 8, 0.1275, 1.0 | none: only their product is fitted to the table above | **unverified** |
| Preload | 39 N.m | 4 kgf.m, the middle of the 3 to 5 kgf.m "initial torque" of Cusco's Type RS clutch-type front LSD for the front-drive Honda Civic Type R FK8 ([Evasive Motorsports listing](https://www.evasivemotorsports.com/store/product/cusco-lsd-type-rs-honda-civic-type-r-fk8-fl5-2017-15-way-45-degree-15-degree/), read 2026-11-22); inside Drexler's 0 to 75 N.m adjustment range and above the 30 to 35 N.m (25 to 30 run in) it sets its Formula Student unit to (same Drexler document) | estimated; the Cusco figure was read on a retailer's listing, **not verified** against Cusco's own sheet |

Not used: Cusco's cam angles (45 / 15 for the FK8 unit), because Cusco
quotes cam angle from a different reference from Drexler's ramp angle,
and no conversion was found.

## Cold tires and temperature (D-108, D-109)

![Cold start](img/cold_start_hatch_tern_gt.svg)

`qts-sim cold-start --svg docs/img/cold_start_hatch_tern_gt.svg` draws
this. It runs the Tern GT from the pits on Ridgeline with the tires at
20 C and at 45 C, driven by the path follower close to the limit (planning
for 8 m/s2). Since D-109 the model follows the published figures below:
grip peaks at 30 C (window 38 C) and stiffness falls 0.006 per degree.
**This curve comes from one tire and is unverified for every tire file
that uses it** (all four do; each file can be tuned on its own).

| Figure | Model | Published | Source |
| --- | --- | --- | --- |
| Peak grip, cold (about 30 C) to hot (about 70 C) surface | -10.0 % (was -1.5 % before D-109) | about -10 %, falling as the tire heats | Singh and Sivaramakrishnan (Goodyear), "Extended Pacejka Tire Model for Enhanced Vehicle Stability Control", [arXiv:2305.18422](https://arxiv.org/abs/2305.18422) (2023), Table 3 and Figure 26: Flat-Trac, high-performance summer tire, new tires without warm-up; read first-hand 2026-11-22 |
| Cornering stiffness, cold to hot | -22.6 % (stiffness factor, carcass 30 to 70 C; was -7.8 %) | -20 to -25 %, "strongly influenced by the tire bulk temperature" | same |
| Warm-up | since D-109 there is none to wait for: from the pits at 20 C the tires grip 3 % better at the first corner than tires started at 45 C, and lose grip where they heat (before D-109: within 0.5 % of warm tires from corner 5 of 11) | no car figure found; Michelin advises motorcyclists to warm tires gently for the first two to three laps | [Michelin, tire pressure on track (motorcycle)](https://www.michelinman.com/motorcycle/tips-and-advice/tips-to-take-care-of-motorcycle-tires/tire-pressure-on-track), seen in search results only, **not verified** |

Not found: a published grip-against-temperature curve for a road summer
tire below 30 C, and a measured warm-up time for road tires on a car.

## Drift judging and the Formula DRIFT rulebook (D-114, D-119)

Source: Formula DRIFT, *Judging Regulations*, 2026 version 1.3
(formulad.com, rulebook PDF, read 23 November 2026), sections 3.1
(qualifying criteria and scoring) and 3.2 (incomplete qualifying runs).

| Rule | Rulebook | Here |
|---|---|---|
| Weights | line 40, angle 40, style 20 | the same |
| Line | share of each zone filled (body outline next to walls, wheelbase otherwise), less any No-Go area beyond it, averaged over the zones | inner zone: nearest body corner to the inner clipping point, full at 0.5 m, none at 4 m. Outer zone: the rear bumper's reach to the zone's line, none 4 m short. Past the line, a No-Go deduction from 0.5 m on, the whole outer half at 4.5 m. Distances stand in for areas |
| Angle | average angle over the run, from the judged start to the finish, divided by a maximum set per event from the field's data | average body slip over every judged sample divided by 35 degrees, a fixed maximum (the rulebook's example uses 50) |
| Style | judged by people: front-wheel control, pace, rate of initiation and transitions, handbrake use, throttle, tyres off outside the zones | automated stand-in: share of time sliding, less 30 % per straighten-and-slide-again |
| Spin | Incomplete, no score for the run ("approximately 180 degrees" of over-rotation, 5.3) | Incomplete above 150 degrees of body slip |
| Opposite drift | Incomplete | Incomplete after 0.5 s sliding the wrong way for the corner (no time given in the rulebook) |
| Tyres off | three tyres off in the zones: Incomplete | three tyres off the course at any judged moment |
| Not in control over the finish, unchaseable lead | Incomplete | not judged |

**Unverified here:** the line and style stand-ins, the 35-degree maximum,
the 150-degree spin and the 0.5 s opposite-drift time are this project's
choices. The rulebook scores line and angle by telemetry (the Universal
Drifting Scoring Method, Race Data Labs) and style by judges.

## Halden E4 and the Tesla Model 3 Performance (2018 to 2020)

Comparable: Model 3 Dual Motor Performance with the Performance Upgrade
(20-inch wheels, Michelin Pilot Sport 4S tires, larger brakes).

| | Halden E4 (data) | Model 3 Performance as tested |
| --- | --- | --- |
| Layout | one motor per axle, all-wheel drive, single reductions | same |
| Power | front 137 kW + rear 213 kW = 350 kW (since D-078; was 160 + 250 = 410 kW) | 450 hp (336 kW) in 2018; about 470 hp after a 2019 update, Car and Driver's estimate |
| Battery | 75 kWh usable | 80.5 kWh |
| Kerb weight | 1850 kg | 4087 lb (1854 kg) (2018 test) |
| Tires | 235/40R19 | 20-inch Michelin Pilot Sport 4S |

Figures:

| Metric | Value | Source |
| --- | --- | --- |
| Skidpad | 0.96 g (diameter not stated) | [Car and Driver, "Tested: 2019 Tesla Model 3 Performance Hits 60 in 3.1 Seconds", Connor Hoffman, 4 May 2021](https://autos.yahoo.com/tested-2019-tesla-model-3-190200890.html) |
| 70-0 mph | 147 ft (44.81 m) | same |
| 0-60 mph | 3.1 s | same |
| Top speed | 162 mph (72.4 m/s), claimed, an electronic limit | same |
| Cross-check | 0.95 g; 70-0 in 147 ft; 0-60 in 3.5 s; "moderate understeer" on the skidpad; governed to 153 mph | [Car and Driver, "2018 Tesla Model 3 Performance Rules the Passing Lane", Kevin A. Wilson, 25 October 2018](https://autos.yahoo.com/2018-tesla-model-3-performance-165000366.html) |

The understeer target (2.0 deg/g) is a guess; the only steering data found
is the qualitative "moderate understeer".

## Steering (D-083)

| Car | Figure | Value | Source |
| --- | --- | --- | --- |
| Tern GT | turns lock to lock | 2.4 (432 degrees each way) | [Carfolio, 2010 MINI Cooper S R56](https://www.carfolio.com/mini-cooper-s-216579) (since D-093; was 2.1 turns from the Golf GTI) |
| Tern GT | turning circle | 10.7 m | BMW Group press technical data, January 2010, seen as a search-result excerpt (since D-093; was 10.9 m) |
| Halden E4 | turns lock to lock, turning circle | 2.00; 11.6 m curb to curb | [Tesla Model 3 owner's manual 2017-2023, Subsystems](https://www.tesla.com/ownersmanual/2017_2023_model3/en_ae/GUID-E414862C-CFA1-4A0B-9548-BE21C32CAA58.html) (newer manuals give 2.14 turns and 11.7 m) |

Ratios are derived from these (D-083): 13.40 (since D-093; was 11.73)
and 10.30.

## Tire sanity (D-086)

Published ranges, from [Wikipedia, "Tire load sensitivity"](https://en.wikipedia.org/wiki/Tire_load_sensitivity)
(read 2026-11-21), which cites Milliken and Milliken, *Race Car Vehicle
Dynamics* (SAE, 1995), p. 27 and figure 2.9:

- production-car tires develop their peak lateral force at 6 to 10 degrees
  of slip angle, and the angle rises with load (in the book's figure,
  5.6, 6.0 and 6.7 degrees at 900, 1350 and 1800 lbf);
- the friction coefficient falls as load rises (1.10, 1.08, 0.97 in that
  figure), while the peak force still rises, roughly as the load to a
  power of 0.7 to 0.9 (marked "citation needed" in the article).

No source was found for the slip ratio of the longitudinal peak; it is
plotted but not judged.

`qts-sim tire-plots` draws, for each tire file, lateral force against slip
angle and longitudinal force against slip ratio at half, one and one and a
half times the reference load, and the combined-slip curves at the
reference load; `crates/qts-sim/tests/tire_sanity.rs` checks the same
curves.

| Tire | Peak slip angle at 0.5 / 1 / 1.5 x load | Peak Fy/Fz | Load exponent | Combined slip, worst | Result |
| --- | --- | --- | --- | --- | --- |
| summer_195_55r16 (Tern GT since D-093) | 8.65 / 9.45 / 10.95 deg | 1.20 / 1.12 / 1.04 | 0.84 | the same coefficients | inside every range |
| summer_225_40r18 (Tern GT before D-093) | 8.65 / 9.45 / 10.95 deg | 1.20 / 1.12 / 1.04 | 0.84 | 0.9997 of the ellipse | inside every range |
| performance_235_40r19 (Halden E4) | 8.65 / 9.45 / 10.95 deg | 1.20 / 1.12 / 1.04 | 0.84 | 0.9997 | inside every range |
| street_205_45r17 (unused since D-078) | 9.05 / 9.65 / 11.15 deg | 1.17 / 1.09 / 1.01 | 0.83 | 0.9998 | inside every range |

Plots: `docs/img/tire_summer_225_40r18.svg`,
`docs/img/tire_performance_235_40r19.svg`, `docs/img/tire_street_205_45r17.svg`.

D-086 found two gaps, closed in D-087 with the owner's approval:

1. **Combined slip** reached 3 to 4 % beyond the ellipse of the pure peaks
   when braking at about 6 degrees of slip angle, in every file. Cause,
   confirmed by a sweep: the combined-slip weight `rbx1`; raised from 12
   to 16.
2. **Street tire peak angle** was 10.15 degrees at its reference load;
   `pky1` raised from 19 to 20 puts it at 9.65.

Effect on validation: skidpad 0.934 to 0.923 g (Tern GT) and 0.955 to
0.953 g (Halden E4); everything else unchanged; all targets pass.

## Results since D-109

`cargo run --release -p qts-validate`, tires starting at their new 30 C
optimum (D-109):

| Car | Metric | Simulated | Target | Error |
| --- | --- | --- | --- | --- |
| Tern GT | skidpad | 0.935 g | 0.87 g | +7.4 % |
| Tern GT | 60-0 mph | 33.82 m | 34.59 m | -2.2 % |
| Tern GT | 0-60 mph | 7.12 s | 6.6 s | +7.8 % |
| Tern GT | top speed | 64.65 m/s | 63.33 m/s | +2.1 % |
| Tern GT | understeer (guess) | 1.90 deg/g | 2.5 | -23.9 % |
| Halden E4 | skidpad | 0.940 g | 0.96 g | -2.1 % |
| Halden E4 | 70-0 mph | 45.61 m | 44.81 m | +1.8 % |
| Halden E4 | 0-60 mph, 1 ft rollout | 2.84 s | 3.1 s | -8.4 % |
| Halden E4 | top speed | 71.52 m/s | 72.42 m/s | -1.2 % |
| Halden E4 | understeer (guess) | 1.85 deg/g | 2.0 | -7.5 % |

Kept at 60 C, as the harness used to start, the hot tires would fail two
targets (Tern GT 0-60 +11.0 %, Halden E4 70-0 +10.9 %).

## Results since D-093

History, before D-109. `cargo run --release -p qts-validate`, with the
Tern GT on the Cooper S data and the torque curve read as net output
(D-093):

| Car | Metric | Simulated | Target | Error |
| --- | --- | --- | --- | --- |
| Tern GT | skidpad | 0.931 g | 0.87 g | +7.0 % |
| Tern GT | 60-0 mph | 34.57 m | 34.59 m | -0.1 % |
| Tern GT | 0-60 mph | 7.17 s | 6.6 s | +8.6 % |
| Tern GT | top speed | 64.65 m/s | 63.33 m/s | +2.1 % |
| Tern GT | understeer (guess) | 1.92 deg/g | 2.5 | -23.4 % |

The Halden E4 is unchanged (electric, so the torque-curve change does not
touch it). The Tern GT's 0-60 is the closest to its tolerance: traction
control holds the launch near the front tires' limit and two shifts fall
before 60 mph. Its skidpad is above a figure taken from the heavier,
softer convertible, as expected. Nothing was tuned to the targets.

## Results, before and after closing the gaps (D-078)

History: these are the Tern GT's figures against the Golf GTI, before
D-093.

`cargo run --release -p qts-validate`. "Before" is D-077, "after" D-078.

| Car | Metric | Before | After | Published | Error before | Error after |
| --- | --- | --- | --- | --- | --- | --- |
| Tern GT | skidpad | 0.910 g | 0.934 g | 0.94 g | -3.2 % | -0.6 % |
| Tern GT | 70-0 mph | 48.42 m | 46.59 m | 46.94 m | +3.2 % | -0.7 % |
| Tern GT | 0-60 mph, rollout | 6.21 s | 6.08 s | 5.9 s | +5.2 % | +3.0 % |
| Tern GT | top speed | 69.68 m/s | 69.99 m/s | 69.44 m/s | +0.3 % | +0.8 % |
| Tern GT | understeer (guess) | 2.20 deg/g | 2.23 deg/g | 2.5 | -12.2 % | -10.8 % |
| Halden E4 | skidpad | 0.956 g | 0.955 g | 0.96 g | -0.4 % | -0.5 % |
| Halden E4 | 70-0 mph | 46.57 m | 46.57 m | 44.81 m | +3.9 % | +3.9 % |
| Halden E4 | 0-60 mph, rollout | 2.72 s | 2.91 s | 3.1 s | **-12.1 %** | -6.3 % |
| Halden E4 | top speed | 71.79 m/s | 71.72 m/s | 72.42 m/s | -0.9 % | -1.0 % |
| Halden E4 | understeer (guess) | 1.83 deg/g | 1.83 deg/g | 2.0 | -8.4 % | -8.4 % |

What changed, each anchored to a published figure rather than fitted to
the targets:

1. **Halden E4 motor power**: 160 + 250 kW to 137 + 213 kW, 350 kW
   together, Car and Driver's estimate of the comparable's output (about
   470 hp), keeping the front-rear proportion. Torque unchanged. The
   0-60 is still 0.2 s quick; matching it exactly would mean tuning to the
   target, which was not done.
2. **Tern GT torque curve**: Volkswagen's published 350 N.m from 1500 to
   4400 rpm and 169 kW from 4700 to 6200 rpm. The ends are guesses.
3. **Tern GT weight**: the comparable's tested kerb weight, 1413 kg; the
   body inertias scale with the sprung mass.
4. **Tern GT tires**: a 225/40 R18 summer tire, the size and class the
   comparable was tested on, derived by size from the existing
   performance-tire placeholder (`data/tires/summer_225_40r18.toml`). The
   compound coefficients are still placeholders.

Each was tried alone first: at the real weight on the old narrow tire the
car moved further from the targets (0-60 +9.8 %, skidpad -5.2 %), which is
what pointed at the tire.

Still open:

- **Halden E4 braking (+3.9 %)**: within tolerance; no published tire or
  brake data to anchor a change, so none was made.
- **Understeer**: no published data for either car.

## What was proposed before the changes

History, before D-078 and D-093.

`cargo run --release -p qts-validate`, after D-077 and with no physics
change:

| Car | Metric | Simulated | Target | Error |
| --- | --- | --- | --- | --- |
| Tern GT | skidpad | 0.910 g | 0.94 g | -3.2 % |
| Tern GT | 70-0 mph | 48.42 m | 46.94 m | +3.2 % |
| Tern GT | 0-60 mph, rollout | 6.21 s | 5.9 s | +5.2 % |
| Tern GT | top speed | 69.68 m/s | 69.44 m/s | +0.3 % |
| Halden E4 | skidpad | 0.956 g | 0.96 g | -0.4 % |
| Halden E4 | 70-0 mph | 46.57 m | 44.81 m | +3.9 % |
| Halden E4 | 0-60 mph, rollout | 2.72 s | 3.1 s | **-12.1 %, outside tolerance** (known gap) |
| Halden E4 | top speed | 71.79 m/s | 72.42 m/s | -0.9 % |

Parameters I would change, none changed yet (physics changes for
validation wait for approval):

1. **Halden E4 acceleration (outside tolerance).** The car is about 0.4 s
   too quick. Its motor peak torque and power were raised from driving
   feedback to reach 60 mph in just under 3 s timed without rollout; with
   rollout subtracted that is 2.7 s. Lower `peak_torque_nm` and
   `peak_power_w` of both motors (`[drivetrain.front_motor]`,
   `.rear_motor`). The 410 kW sum of motor peaks is also well above the
   336 to 350 kW the comparable is quoted at. A first try would be about
   15 % less peak power, then re-run validation; the right amount has to
   come from running it, not from this estimate.
2. **Tern GT acceleration (+5 %, inside tolerance).** The comparable makes
   350 N.m from 1500 rpm against our 320 N.m plateau from 2000 rpm, and is
   heavier (1413 kg against 1300 kg). Raising the low and mid-range of
   `torque_curve_nm` toward 350 N.m would close most of it; the kerb
   weight (`body.sprung_mass_kg`) should rise toward the comparable's at
   the same time, which works the other way. Launch traction on the
   narrower 205 tire also limits the first metres.
3. **Tern GT skidpad and braking (-3 %, +3 %).** The comparable runs
   225/40R18 summer tires with a limited-slip differential; ours run
   205/45R17. Moving `tire_front`/`tire_rear` to a wider tire, or raising
   the street tire's friction coefficients (`pdy1`, `pdx1`) a few percent,
   would close both. Comparing against `hatch_tern_gt_lsd` would match the
   differential; it has no target file yet.
4. **Halden E4 braking (+3.9 %).** Either the longitudinal friction of the
   performance tire (`pdx1`) or the anti-lock slip target
   (`brakes.abs.slip_threshold`) is a little low for a Pilot Sport 4S
   class tire.
5. **Understeer.** No published data for either car. A steady-state
   steering-angle sweep from a magazine or a measured run would be needed.

## Kestrel RC handbrake torque (D-124)

The placeholder 2,200 N·m per rear wheel is replaced by 1,300 N·m, worked
out from parts a drift FC RX-7 (the Kestrel's comparable car) would carry:
a hydraulic handbrake lever feeding the stock rear calipers.

| Input | Value | Source | Confidence |
|---|---|---|---|
| Hand pull at full travel | 445 N (100 lb) | handbrake example in the Mark Williams brake pedal calculator; Tolomatic's master cylinder selection guide gives 75 lb as a design figure for lever effort | estimated |
| Lever ratio | 11:1 | Wilwood hydraulic handbrake lever ("11:1 ratio H beam lever"), Wilwood flyer fl451 | published, for that lever |
| Master cylinder bore | 3/4 in (2.85 cm²) | the bore hydraulic handbrake kits offer for OEM calipers (Voodoo13 HYHB-0101; a Wilwood handbrake listing with a 3/4 in compact master cylinder) | published, for those kits |
| Line pressure | 17.2 MPa (2,490 psi) | 445 N × 11 / 2.85 cm² | worked out |
| Rear caliper | one 33 mm piston, floating | a parts listing for an FC Turbo rear caliper (BuyBrakes, Dynamic Friction 331-80607); not a factory figure | single source, not verified |
| Rear rotor | 273 mm | several FC rear rotor listings (Centric 121.45024 at 272.9 mm, PowerStop, RDA); one fitment table gives 294 mm for 1989-91, which other listings give for the FD | consistent across listings |
| Pad friction | 0.40 | typical street pad | estimated |
| Effective radius | 112 mm | rotor radius less half a 40 mm pad, rounded down | estimated |

Torque per wheel = 2 × 0.40 × 17.2 MPa × 8.55 cm² × 0.112 m ≈ 1,315 N·m,
rounded to 1,300. A 75 lb pull gives about 990 N·m.

Effect in the simulation:
- **Clutch down, as drifters pull it:** both rears lock at 14 m/s in second
  gear while the fronts roll (`drift_car.rs`).
- **Clutch engaged:** the engine holds them at 20 rad/s.
- **Lower figures:** an earlier pass with a 5:1 lever (my guess, before
  Wilwood's 11:1 was found) gave 600 N·m, which locked nothing. The rears
  here need about 1,100 N·m to lock in a straight line, so a weak pull on a
  lower-ratio lever would not lock them.

Sources:
- Wilwood hydraulic handbrake flyer: https://www.wilwood.com/Pdf/Flyers/fl451.pdf
- Wilwood hydraulic handbrake levers: https://www.wilwood.com/Media/BrakingNewsDetail?id=100
- Voodoo13 hydraulic handbrake: https://www.maperformance.com/products/voodoo13-hydraulic-handbrake-hyhb-0101
- Mark Williams brake pedal calculator: https://www.markwilliams.com/brakepedalcalculator.html
- Tolomatic, master cylinder selection: https://tolomatic.com/wp-content/uploads/2024/10/MasterCylinder_Selection.pdf
- FC rear caliper listing: https://www.buybrakes.com/dynamic-friction-331-80607-premium-silver-zinc-coated-caliper
- FC rear rotor: https://www.maperformance.com/products/centric-rear-standard-brake-rotor-1986-1991-mazda-rx-7-121-45024
- Rear rotor fitment table: https://cararac.com/brake-discs/mazda/rx-7.html

## The weave after a slide (D-124)

No published frequency was found for a car's hands-off oscillation. ISO
17288-1 (steering release, free-control behaviour) defines the test but its
results are not public, and the search turned up none. Hands-off steering
patents rate a yaw damping ratio under about 0.1 as poor. So it was checked
inside the model instead (`drift_ffb.rs`):

- **Steering held:** the car does not oscillate. The car alone is stable.
- **Hands off, with the preset's force-feedback damping of 0.03
  N·m·s/rad:** a lasting yaw oscillation, about 0.8 Hz (1.9 Hz at 0.1), with
  the throttle closed and with it open. The loop between the force feedback
  and a free, light rim (0.02 kg·m², estimated) is what oscillates.
- **Damping:** base friction up to 0.4 N·m does not stop it. Force-feedback
  damping of 0.2 N·m·s/rad does.

Physics is unchanged. Whether a real CSL Elite weaves depends on its own
internal damping, which is not published. The preset's damping is left for
the F5 force-feedback logs, along with the gain.

- ISO 17288-1 (steering release): https://knowledge.bsigroup.com/products/passenger-cars-free-steer-behaviour-steering-release-open-loop-test-method
