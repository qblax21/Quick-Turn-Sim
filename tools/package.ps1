# SPDX-License-Identifier: Apache-2.0
# Builds a standalone folder that runs without Rust or the Godot editor
# project: the executable, the game pack, the simulation library and the
# data files.
#
#   powershell -ExecutionPolicy Bypass -File tools\package.ps1
#   powershell -ExecutionPolicy Bypass -File tools\package.ps1 -Godot C:\path\to\godot.exe
#
# With Godot's export templates installed (Editor > Manage Export Templates)
# the folder holds a proper release executable. Without them the script
# falls back to a development package that uses a copy of the Godot editor
# binary as the runtime: it plays the same, but it is larger and is not what
# should be shipped.

param(
    [string]$Godot = "",
    [string]$Out = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# The product name lives in one constants file; the folder and executable
# take their name from it.
$constants = Get-Content (Join-Path $root "crates\qts-core\src\constants.rs") -Raw
if ($constants -notmatch 'PRODUCT_NAME: &str = "([^"]+)"') {
    Write-Error "Could not read the product name from constants.rs."
}
$name = $Matches[1] -replace '[^A-Za-z0-9]', ''

if (-not $Godot) {
    # Prefer the console executable: the windowed one prints nothing, so its
    # version cannot be read.
    $found = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter "Godot_v*_win64_console.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($found) {
        $Godot = $found.FullName
    } else {
        $command = Get-Command godot -ErrorAction SilentlyContinue
        if ($command) { $Godot = $command.Source }
    }
}
if (-not $Godot -or -not (Test-Path $Godot)) {
    Write-Error "Godot was not found. Pass its path with -Godot."
}

cargo build --release -p qts-godot
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$library = Join-Path $root "target\release\qts_godot.dll"
New-Item -ItemType Directory -Force -Path (Join-Path $root "godot\bin") | Out-Null
Copy-Item -Force $library (Join-Path $root "godot\bin")

if (-not $Out) { $Out = Join-Path $root "out" }
$dist = Join-Path $Out $name
if (Test-Path $dist) { Remove-Item -Recurse -Force -Confirm:$false $dist }
New-Item -ItemType Directory -Force -Path $dist | Out-Null

# "4.7.2.stable.official.abcdef" -> "4.7.2.stable"
$reported = & $Godot --version
$first = $reported | Where-Object { $_ -match '^\d' } | Select-Object -First 1
$version = "$first" -replace '^(\d+(\.\d+)+\.[a-z0-9]+).*$', '$1'
$template = Join-Path $env:APPDATA "Godot\export_templates\$version\windows_release_x86_64.exe"
$project = Join-Path $root "godot"

if (Test-Path $template) {
    & $Godot --headless --path $project --export-release "Windows Desktop" (Join-Path $dist "$name.exe") | Out-Null
    $kind = "release"
} else {
    & $Godot --headless --path $project --export-pack "Windows Desktop" (Join-Path $dist "$name.pck") | Out-Null
    # The windowed engine binary sits beside the console one.
    $engine = $Godot -replace '_console\.exe$', '.exe'
    Copy-Item -Force $engine (Join-Path $dist "$name.exe")
    $kind = "development"
}
if (-not (Test-Path (Join-Path $dist "$name.pck"))) {
    Write-Error "The export did not produce $name.pck."
}

Copy-Item -Force $library $dist
Copy-Item -Recurse -Force (Join-Path $root "data") (Join-Path $dist "data")
foreach ($file in @("LICENSE", "LICENSE-ASSETS", "README.md")) {
    Copy-Item -Force (Join-Path $root $file) $dist
}

Write-Host ""
Write-Host "Packaged a $kind build in $dist"
Write-Host "Run $name.exe from that folder."
if ($kind -eq "development") {
    Write-Host ""
    Write-Host "Export templates for Godot $version are not installed, so this folder"
    Write-Host "uses a copy of the Godot editor binary as its runtime. Install them from"
    Write-Host "the editor (Editor > Manage Export Templates) and run this script again"
    Write-Host "to get a release executable."
}
