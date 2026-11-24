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

#
# -Demo builds the demo slice (D-115): two cars, two tracks, the first-run
# wizard, the library from the stripped "dist" profile and the "Windows
# Demo" export preset. It zips the folder and prints the download and
# install sizes. With -CheckBudget it fails if the parts this project
# ships (game pack, library, data; the Godot runtime is fixed per engine
# version) go over the budget in tools\demo_budget.txt.

param(
    [string]$Godot = "",
    [string]$Out = "",
    [switch]$Demo,
    [switch]$CheckBudget
)

# The demo slice: a drift car and a grip car, a drift track and a circuit.
$demoCars = @("coupe_kestrel_rc", "hatch_tern_gt")
$demoTracks = @("harrow_drift_park", "ridgeline_circuit")

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

$cargoProfile = if ($Demo) { "dist" } else { "release" }
cargo build --profile $cargoProfile -p qts-godot
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$library = Join-Path $root "target\$cargoProfile\qts_godot.dll"
New-Item -ItemType Directory -Force -Path (Join-Path $root "godot\bin") | Out-Null
Copy-Item -Force $library (Join-Path $root "godot\bin")

if (-not $Out) { $Out = Join-Path $root "out" }
if ($Demo) { $name = "${name}Demo" }
$dist = Join-Path $Out $name
$preset = if ($Demo) { "Windows Demo" } else { "Windows Desktop" }
if (Test-Path $dist) { Remove-Item -Recurse -Force -Confirm:$false $dist }
New-Item -ItemType Directory -Force -Path $dist | Out-Null

# "4.7.2.stable.official.abcdef" -> "4.7.2.stable"
$reported = & $Godot --version
$first = $reported | Where-Object { $_ -match '^\d' } | Select-Object -First 1
$version = "$first" -replace '^(\d+(\.\d+)+\.[a-z0-9]+).*$', '$1'
$template = Join-Path $env:APPDATA "Godot\export_templates\$version\windows_release_x86_64.exe"
$project = Join-Path $root "godot"

if (Test-Path $template) {
    & $Godot --headless --path $project --export-release $preset (Join-Path $dist "$name.exe") | Out-Null
    $kind = "release"
} else {
    & $Godot --headless --path $project --export-pack $preset (Join-Path $dist "$name.pck") | Out-Null
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
if ($Demo) {
    # Only the demo's cars and tracks; the validation targets are for tests.
    Remove-Item -Recurse -Force -Confirm:$false (Join-Path $dist "data\validation")
    foreach ($pair in @(@("cars", $demoCars), @("tracks", $demoTracks))) {
        Get-ChildItem (Join-Path $dist "data\$($pair[0])") -Filter *.toml |
            Where-Object { $pair[1] -notcontains $_.BaseName } |
            Remove-Item -Force -Confirm:$false
    }
}
foreach ($file in @("LICENSE", "LICENSE-ASSETS", "README.md")) {
    Copy-Item -Force (Join-Path $root $file) $dist
}

if ($Demo) {
    $zip = Join-Path $Out "$name.zip"
    if (Test-Path $zip) { Remove-Item -Force -Confirm:$false $zip }
    Compress-Archive -Path $dist -DestinationPath $zip -CompressionLevel Optimal
    $files = Get-ChildItem -Recurse -File $dist
    $install = ($files | Measure-Object Length -Sum).Sum
    $runtime = (Get-Item (Join-Path $dist "$name.exe")).Length
    $shipped = $install - $runtime
    $mb = { param($b) "{0:N1} MB" -f ($b / 1MB) }
    Write-Host ""
    Write-Host "Demo download (zip): $(& $mb (Get-Item $zip).Length)"
    Write-Host "Demo install:        $(& $mb $install)"
    foreach ($f in $files | Sort-Object Length -Descending | Select-Object -First 5) {
        Write-Host ("  {0,-28} {1}" -f $f.Name, (& $mb $f.Length))
    }
    Write-Host "Shipped by this project (all but the Godot runtime): $(& $mb $shipped)"
    if ($CheckBudget) {
        $budget = [double]((Get-Content (Join-Path $root "tools\demo_budget.txt") | Where-Object { $_ -match '^\s*\d' } | Select-Object -First 1).Trim())
        if ($shipped / 1MB -gt $budget) {
            Write-Error ("Over the demo size budget: {0:N1} MB shipped, budget {1} MB." -f ($shipped / 1MB), $budget)
        }
        Write-Host "Within the budget of $budget MB."
    }
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
