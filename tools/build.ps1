# SPDX-License-Identifier: Apache-2.0
# Builds the whole workspace and puts the binding library where the Godot
# project expects it.
#
#   powershell -ExecutionPolicy Bypass -File tools\build.ps1          release build
#   powershell -ExecutionPolicy Bypass -File tools\build.ps1 -Test    build, then run every test
#   powershell -ExecutionPolicy Bypass -File tools\build.ps1 -Debug   debug build

param(
    [switch]$Test,
    [switch]$Debug
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "cargo was not found. Install Rust from https://rustup.rs and open a new terminal."
}

$profileArgs = @()
$profileDir = "debug"
if (-not $Debug) {
    $profileArgs = @("--release")
    $profileDir = "release"
}

cargo build --workspace @profileArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$library = Join-Path $root "target\$profileDir\qts_godot.dll"
$destination = Join-Path $root "godot\bin"
New-Item -ItemType Directory -Force -Path $destination | Out-Null
Copy-Item -Force $library $destination
Write-Host "Copied $library to $destination"

if ($Test) {
    cargo test --workspace @profileArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Write-Host ""
Write-Host "Done. Open the 'godot' folder in Godot 4.4 or later and press Play,"
Write-Host "or run the headless simulator: target\$profileDir\qts-sim.exe help"
