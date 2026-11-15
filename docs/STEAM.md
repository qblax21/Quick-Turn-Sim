<!-- SPDX-License-Identifier: Apache-2.0 -->
# Steam: planned integration (nothing is integrated yet)

The foundation contains no Steam code, no Steamworks SDK and no dependency
on either. This note records the intended route so that nothing built now
closes it off.

## Plan

Use **GodotSteam** as a GDExtension in the Godot front end. GodotSteam
wraps the Steamworks SDK and is loaded by Godot the same way the simulation
binding is, as a separate library listed in its own `.gdextension` file.

- Steam features (overlay, achievements, rich presence, Steam Input, cloud
  saves, workshop, lobbies) are front-end and platform concerns. They stay
  in `godot/`, behind a small GDScript wrapper so the game runs unchanged
  when Steam is absent.
- The Rust simulation core never links against Steamworks. It stays
  buildable and testable headless, on CI, without the SDK.
- Networking for ranked play would sit behind the interfaces in
  `qts_core::net` (input frames, snapshots, tick alignment). A Steam
  transport would be one implementation of that, in a layer above the core.

## What the foundation already does to keep the path open

- The front end is a standard Godot 4 project using GDExtension, which is
  what GodotSteam's GDExtension build targets.
- Nothing is Windows-only outside `qts-ffb::platform`, so Linux and Steam
  Deck builds are not blocked by the core. The workspace type-checks for
  Linux in development and CI builds and tests it.
- Input is read in Godot, so Steam Input can be added there without
  touching the simulation.
- Recordings and settings use Godot's `user://` directory, which is the
  place Steam Cloud would be pointed at.
- The product name is defined in one constants file, and file formats do
  not contain it, so a store name change is cheap.

## Open questions for later

- **Steam Deck force feedback and input**: wheels are uncommon there, but
  gamepad rumble and gyro steering would go through Steam Input.
- **Licence check**: GodotSteam is MIT-licensed; the Steamworks SDK has its
  own terms and is not redistributable in a public repository. The SDK
  binaries would be added at packaging time, not committed.
- **Linux force-feedback output** is not implemented. It is needed before a
  Linux release that supports wheels.
- **Anti-cheat and ranked play** are explicitly out of scope for the
  foundation and need their own design.
