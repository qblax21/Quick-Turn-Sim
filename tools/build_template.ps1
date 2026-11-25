# SPDX-License-Identifier: Apache-2.0
# Builds the slimmed Godot 4.7.2 Windows release template (D-121) from
# source, with the modules the game doesn't use left out
# (tools\godot_template\custom.py).
#
#   powershell -ExecutionPolicy Bypass -File tools\build_template.ps1
#
# Needs git, Python with SCons (py -m pip install scons) and Visual Studio
# with the C++ tools and a Windows SDK. CI builds it on every profile change
# (the "godot template" job), so locally you can download that job's
# artifact instead. Output: out\godot-template\godot_template_release.exe
param(
    [string]$Source = "",
    [string]$Out = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$tag = "4.7.2-stable"
if (-not $Source) { $Source = Join-Path $root "out\godot-src" }
if (-not $Out) { $Out = Join-Path $root "out\godot-template" }

if (-not (Test-Path (Join-Path $Source "SConstruct"))) {
    git clone --depth 1 --branch $tag https://github.com/godotengine/godot.git $Source
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

$profile = Join-Path $root "tools\godot_template\custom.py"
$jobs = [Environment]::ProcessorCount
Push-Location $Source
try {
    # Windows platform flags override a profile; only the command line wins.
    python -m SCons "profile=$profile" "d3d12=no" "-j$jobs"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
    Pop-Location
}

$built = Join-Path $Source "bin\godot.windows.template_release.x86_64.exe"
if (-not (Test-Path $built)) { Write-Error "SCons did not produce $built." }
New-Item -ItemType Directory -Force -Path $Out | Out-Null
Copy-Item -Force $built (Join-Path $Out "godot_template_release.exe")
Write-Host ("Template: {0:N1} MB" -f ((Get-Item $built).Length / 1MB))
