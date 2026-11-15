#!/usr/bin/env sh
# SPDX-License-Identifier: Apache-2.0
# Builds the whole workspace and puts the binding library where the Godot
# project expects it. Linux counterpart of tools/build.ps1.
#
#   tools/build.sh           release build
#   tools/build.sh --test    build, then run every test
set -eu
cd "$(dirname "$0")/.."
cargo build --workspace --release
mkdir -p godot/bin
cp target/release/libqts_godot.so godot/bin/
echo "Copied target/release/libqts_godot.so to godot/bin"
if [ "${1:-}" = "--test" ]; then
    cargo test --workspace --release
fi
