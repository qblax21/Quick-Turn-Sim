<!-- SPDX-License-Identifier: Apache-2.0 -->
# Contributing

Thank you for helping. This project values correctness and honesty about
what is and is not validated over features.

## Before you open a pull request

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three must be clean. CI runs the same commands on Windows and Linux.
If you changed the front end, also run the smoke test:

```
godot --headless --path godot --script res://tests/smoke.gd
```

## Ground rules

### Simulation code is deterministic

Code that runs inside a simulation step (`qts-core`, `qts-tire`,
`qts-vehicle`, and the `session` and `track` modules of `qts-sim`) must be a
pure function of state, input and tick:

- No clocks, no random numbers, no global or thread-local state.
- No `HashMap` or `HashSet` (clippy enforces this).
- Transcendental math goes through `qts_core::math::fm`, not the `f64`
  methods.
- Anything a step reads on the next tick must be in the snapshot. If you
  add state, add it to `save` and `load` in the same change; the hash is
  taken over the same bytes.

The determinism, replay and snapshot tests will catch most violations.

### Parameters live in data files, with provenance

- No physical parameter in code. Add it to the parameter struct and to the
  files in `data/`.
- Every number in a car, tire, surface or force-feedback file is written as
  `{ value = ..., source = "...", confidence = "..." }`. The loader rejects
  anything else.
- `confidence` is `placeholder`, `estimated` or `measured`. Use `measured`
  only for values taken from a measurement the project can point to, and
  say where in `source`.
- Do not invent data. A guess is fine if it is labelled as one.

### No realism claims without a test

Do not describe a model as realistic or accurate in code, documents or UI
unless a validation test against `measured` targets passes. Describe what
the model does and what it was checked against.

### No real cars, brands or licensed content

Cars, tracks and their names are original. Do not add real vehicle names,
logos, liveries, circuit layouts or data copied from other games or
licensed sources. Naming compatible hardware (a wheel base in a preset) is
fine.

### Keep the layers apart

- No simulation logic in `godot/` or in `qts-godot`. The binding converts
  types and axes.
- Platform-specific code goes in `qts-ffb::platform` only.
- The product name is written in `crates/qts-core/src/constants.rs` and
  `README.md` only. Read it from the constants everywhere else.

### Dependencies

Adding a dependency needs a reason recorded in `DECISIONS.md`. Prefer a few
dozen lines of plain code to a new crate, especially in the simulation
path.

## Decisions and status

- Record non-obvious choices in `DECISIONS.md` (what, and why).
- If your change makes something work that `STATUS.md` lists as stubbed or
  untuned, or the reverse, update `STATUS.md`.

## Tests to add with a change

| You changed | Add or update |
| --- | --- |
| Tire equations | A reference value from `tools/mf_reference.ps1` or an analytic check |
| Vehicle dynamics | A physical check (load transfer, energy, direction of a response) |
| State layout | Nothing extra if `save`/`load` are updated; the existing tests cover it |
| Force-feedback pipeline | A unit test on the signal |
| A data file | The content tests load every file; add a check if it has a new field |
| A validation manoeuvre | Document its definition in `DECISIONS.md` |

## Licensing of contributions

Code contributions are licensed under Apache-2.0 and data or asset
contributions under CC BY 4.0, matching the repository. Start each new
source file with the matching `SPDX-License-Identifier` line.
