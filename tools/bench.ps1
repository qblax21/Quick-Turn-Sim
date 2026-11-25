# SPDX-License-Identifier: Apache-2.0
# Runs the render bench from the repository (D-125) and prints where the
# results file went. In a packaged build, use "Run render bench.bat".
#
#   powershell -ExecutionPolicy Bypass -File tools\bench.ps1 [-Preset all|Low|High] [-LowEnd] [-Seconds N]
param(
    [string]$Preset = "all",
    [switch]$LowEnd,
    [int]$Seconds = 0,
    [string]$Godot = ""
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not $Godot) {
    $found = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter "Godot_v*_win64_console.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($found) { $Godot = $found.FullName }
}
if (-not $Godot -or -not (Test-Path $Godot)) { Write-Error "Godot was not found. Pass its path with -Godot." }
$engineArgs = @("--path", (Join-Path $root "godot"))
$userArgs = @("--render-bench=$Preset")
if ($LowEnd) {
    # A second GPU if there is one; Godot falls back to the first otherwise.
    $engineArgs += @("--gpu-index", "1")
    $userArgs += "--bench-low-end"
}
if ($Seconds -gt 0) { $userArgs += "--bench-seconds=$Seconds" }
& $Godot @engineArgs -- @userArgs | Where-Object { $_ -match "render bench" }
