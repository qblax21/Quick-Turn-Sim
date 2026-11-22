# SPDX-License-Identifier: Apache-2.0
# Rebuilds the drawn car models with Blender: for every car data file that
# is not a variant (no `inherits`), writes godot/assets/cars/<id>.glb, the
# editable art/cars/<id>.blend and preview renders in docs/cars/.
# Editing a .blend by hand and then running this overwrites it.
#   .\tools\make_car_models.ps1 [-Blender C:\path\to\blender.exe]
param([string]$Blender = $env:BLENDER)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not $Blender) {
    $cmd = Get-Command blender -ErrorAction SilentlyContinue
    if ($cmd) { $Blender = $cmd.Source }
}
if (-not $Blender) {
    $found = Get-ChildItem "C:\Program Files\Blender Foundation" -Recurse -Filter blender.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($found) { $Blender = $found.FullName }
}
if (-not $Blender -or -not (Test-Path $Blender)) {
    throw "Blender not found: pass -Blender C:\path\to\blender.exe or set BLENDER"
}
foreach ($car in Get-ChildItem "$root\data\cars\*.toml") {
    if (Select-String -Path $car.FullName -Pattern '^inherits\s*=' -Quiet) { continue }
    $id = $car.BaseName
    Write-Host "Building $id"
    & $Blender -b --factory-startup --python-exit-code 1 -P "$root\tools\blender\make_car.py" -- `
        --car $car.FullName --out "$root\godot\assets\cars\$id.glb" `
        --blend "$root\art\cars\$id.blend" --render "$root\docs\cars\$id.jpg" | Select-String "QTS:|Error|Traceback"
    if ($LASTEXITCODE -ne 0) { throw "Blender failed on $id" }
}
