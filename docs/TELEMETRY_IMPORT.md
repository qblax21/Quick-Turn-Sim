<!-- SPDX-License-Identifier: Apache-2.0 -->
# Importing logger telemetry

This is the path for bringing real measurements into the project, for
example autocross runs logged in the cars that the placeholder models are
meant to be calibrated toward. It turns a logger's CSV export into the
numbers the validation harness compares against.

## The short version

```powershell
target\release\qts-sim.exe import my_run.csv --profile generic --targets
```

The command prints which columns it used, a summary of the run, and (with
`--targets`) entries ready to paste into a file in `data/validation/`.

## What you need from the logger

A CSV export with one row per sample and a header row naming the columns.
The importer needs a time column. Everything else is optional, and a metric
is simply reported as "not found" if its channel or manoeuvre is missing.

| To get | The run must contain | Channel needed |
| --- | --- | --- |
| Sustained lateral g | At least one second of steady cornering | Lateral acceleration |
| 60-0 mph distance | A stop from above 60 mph to rest | Speed |
| 0-60 mph time | A start from rest to above 60 mph | Speed |
| Top speed | Whatever the run reached | Speed |
| Sustained braking g | Half a second of hard braking | Longitudinal acceleration |

An autocross run usually gives a useful lateral figure and little else.
Skidpad laps, a straight-line launch and a straight-line stop give the rest.

## Column-mapping profiles

Loggers differ in column names, units and sign conventions, so the importer
reads a profile from `data/import/<name>.toml` that says which column feeds
which canonical channel and how to convert it to SI.

`data/import/generic.toml` lists common names. It is a starting point, not
a specification of any logger: open your export, read its header row, and
copy the profile if the names differ.

```toml
name = "my_logger"
description = "Export from my logger, firmware 1.2"
delimiter = ","
skip_lines = 3            # lines before the header row (titles, metadata)
skip_after_header = 1     # lines between the header and the data (a units row)
comment_prefix = "#"      # optional: ignore lines starting with this

[[channel]]
name = "time_s"                       # canonical channel
columns = ["Time", "Elapsed (s)"]     # header names to look for, any case
scale = 1.0                           # multiply by this to reach SI

[[channel]]
name = "speed_m_s"
columns = ["GPS Speed (km/h)"]
scale = 0.2777777777777778

[[channel]]
name = "accel_lat_m_s2"
columns = ["Lateral G"]
scale = -9.80665                      # negative if the logger is positive to the right
```

Rules:

- Header names are compared without regard to case, surrounding spaces or
  quotes.
- Several `[[channel]]` entries may feed the same canonical channel (speed
  in km/h and in mph, say). The first one whose column exists is used.
- `value_SI = raw × scale + offset`. `offset` defaults to zero.
- Rows whose time is missing, unreadable or not increasing are skipped and
  counted.

### Canonical channels

| Name | Meaning | Sign |
| --- | --- | --- |
| `time_s` | Time in seconds (required) | |
| `speed_m_s` | Speed over the ground | |
| `accel_lat_m_s2` | Lateral acceleration | Positive to the left |
| `accel_long_m_s2` | Longitudinal acceleration | Positive forward |
| `yaw_rate_rad_s` | Yaw rate | Positive counter-clockwise |
| `steering_wheel_rad` | Steering-wheel angle | Positive left |
| `throttle`, `brake` | Pedals, 0 to 1 | |
| `engine_rpm` | Engine speed | |
| `distance_m` | Distance travelled | |
| `latitude_deg`, `longitude_deg` | Position | |

The summary uses the magnitude of lateral acceleration, so its sign does
not affect the skidpad figure. It does matter for anything that later
compares traces.

## How the summary numbers are computed

- **Sustained lateral g**: the highest mean of |lateral acceleration| over
  any one-second window. A short spike does not count.
- **Sustained braking g**: the highest mean deceleration over any
  half-second window.
- **0-60 mph**: from the last sample at rest (below 0.5 m/s, extended back
  to zero speed using the acceleration over the next half second) to the
  moment speed crosses 26.82 m/s, with no drop of more than 1 m/s on the
  way. The shortest such run in the file. No rollout allowance.
- **60-0 mph distance**: speed integrated over time from the downward
  crossing of 26.82 m/s to rest, with no rise of more than 1 m/s on the
  way. The shortest such stop in the file.

Check these definitions against how your reference numbers were taken. The
simulated metrics are defined in `DECISIONS.md` (D-035); the braking
manoeuvre there applies the pedal at 60 mph, whereas a logged stop that
began above 60 mph is already at full deceleration when it passes 60 mph and
will be shorter for that reason.

## Using the result

1. Paste the generated `[[target]]` entries into `data/validation/<car>.toml`,
   replacing the placeholder with the same `metric`.
2. Keep `confidence = "measured"` and make `source` say what the file was:
   date, place, surface, tires, anything needed to find it again.
3. Choose `tolerance_percent` for each target.
4. Run `target\release\qts-validate.exe`. It will now report the error of
   the placeholder model against your measurement, and probably fail. That
   failure is the calibration to-do list.
5. Adjust parameters in `data/cars` and `data/tires`, changing each
   parameter's `source` and `confidence` as you replace guesses.

The content test `nothing_claims_to_be_measured_and_untuned_things_say_so`
and the validation test `shipped_targets_are_all_labelled_as_not_measured`
assert that nothing is marked measured yet. Update them in the same change
that adds the first real measurement.

## Limits

- The importer handles delimited text with one header row. Binary logger
  formats must be exported to CSV by the logger's own software first.
- It does not resample, filter or align channels with different rates.
- GPS position is read but not used yet.
