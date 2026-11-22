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
- **Understeer gradient**: neither source publishes one. Both targets stay
  `placeholder`.

## Tern GT and the Volkswagen Golf GTI (seventh generation)

Comparable: Golf GTI Mk7, six-speed manual, with the Performance Pack
(limited-slip front differential, larger brakes).

| | Tern GT (data) | Golf GTI as tested |
| --- | --- | --- |
| Layout | front engine, front drive, 6-speed manual | same |
| Engine | 2.0 turbo, 350 N.m from 1500 to 4400 rpm, 169 kW from 4700 to 6200 rpm (since D-078; was a 320 N.m plateau) | 2.0 turbo, 258 lb-ft (350 N.m) at 1500 rpm, 220 hp (164 kW) US; 230 PS (169 kW), 350 N.m from 1500 to 4400 rpm in Europe |
| Differential | open (the `hatch_tern_gt_lsd` variant has a limited-slip) | electronically controlled limited-slip |
| Kerb weight | 1413 kg (since D-078; was 1300 kg) | 3116 lb (1413 kg) (2017 test) |
| Tires | 225/40R18 summer placeholder (since D-078; was 205/45R17) | 225/40R18 Bridgestone Potenza S001 (2015 test) |

Figures:

| Metric | Value | Source |
| --- | --- | --- |
| Skidpad | 0.94 g, 300-ft circle | [Car and Driver, "2017 Volkswagen Golf GTI Manual", Joseph Capparella, 30 March 2017](https://autos.yahoo.com/2017-volkswagen-golf-gti-manual-224500478.html) |
| 70-0 mph | 154 ft (46.94 m) | same |
| 0-60 mph | 5.9 s | same |
| Top speed | 250 km/h (GTI Performance, 230 PS); 246 km/h (220 PS) | [Volkswagen, New Golf GTI international press presentation, 2013](https://www.volkswagen-newsroom.com/en/new-golf-gti-international-press-presentation-2976) (manufacturer figures; the US cars tested were governed at 125 to 126 mph) |
| Cross-check | 0.93 g; 70-0 in 156 ft; 0-60 in 6.1 s (new), 6.2 s at 40,000 miles | [Car and Driver, 2015 Volkswagen GTI long-term test, K.C. Colwell and Tony Quiroga, November 2016 issue](https://autos.yahoo.com/2015-volkswagen-gti-223500648.html) |

The understeer target (2.5 deg/g) is a guess; no source was found.

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
| Tern GT | turns lock to lock | 2.1 (378 degrees each way) | [Volkswagen newsroom, Golf GTI Mk7 running gear](https://www.volkswagen-newsroom.com/en/new-golf-gti-international-press-presentation-2976/new-golf-gti-running-gear-layout-and-electronics-2988) ("just over two full turns", read first-hand); 2.1 turns / 380 degrees in another Volkswagen release, quoted in search results |
| Tern GT | turning circle | 10.9 m | [Volkswagen newsroom, Golf GTI facts in key words](https://www.volkswagen-newsroom.com/en/new-golf-gti-international-press-presentation-2976/new-golf-gti-overview-facts-in-key-words-3015) |
| Halden E4 | turns lock to lock, turning circle | 2.00; 11.6 m curb to curb | [Tesla Model 3 owner's manual 2017-2023, Subsystems](https://www.tesla.com/ownersmanual/2017_2023_model3/en_ae/GUID-E414862C-CFA1-4A0B-9548-BE21C32CAA58.html) (newer manuals give 2.14 turns and 11.7 m) |

Ratios are derived from these (D-083): 11.73 and 10.30. Driven at full
lock, the cars trace 11.12 m and 11.81 m.

## Results, before and after closing the gaps (D-078)

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
