# SPDX-License-Identifier: Apache-2.0
# The store page kit in one command (D-129): records the computer driver's
# laps as replays, renders the five 1920x1080 screenshots and a trailer at
# 1080p60 from them (in-game footage only, through the replay director),
# and encodes the trailer with ffmpeg.
#
#   powershell -ExecutionPolicy Bypass -File tools\steam_kit.ps1 [-Out out\steam_kit] [-Step 1]
#
# Needs tools\build.ps1 run first, Godot 4.7.2 and ffmpeg on the PATH (or
# -Godot / -Ffmpeg). -Step N renders every Nth trailer frame, for a quick
# look; the video then runs N times fast. -StillsOnly renders only the
# screenshots, and -Preset "Very Low"|Low|Medium|High (with -OpenGL for Very Low) renders with that graphics
# preset (D-132 compares presets this way).
param(
    [string]$Out = "",
    [string]$Godot = "",
    [string]$Ffmpeg = "ffmpeg",
    [int]$Step = 1,
    [switch]$StillsOnly,
    [string]$Preset = "",
    [switch]$OpenGL
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not $Out) { $Out = Join-Path $root "out\steam_kit" }
# Godot runs from the project folder: every path it is given is absolute.
if (-not [System.IO.Path]::IsPathRooted($Out)) { $Out = Join-Path (Get-Location) $Out }
$Out = [System.IO.Path]::GetFullPath($Out)
if (Test-Path $Out) { Remove-Item -Recurse -Force -Confirm:$false $Out }
$replays = Join-Path $Out "replays"
New-Item -ItemType Directory -Force -Path $replays | Out-Null
$sim = Join-Path $root "target\release\qts-sim.exe"
if (-not (Test-Path $sim)) { Write-Error "Run tools\build.ps1 first." }
if (-not $Godot) {
    $found = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter "Godot_v*_win64_console.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($found) { $Godot = $found.FullName }
}
if (-not $Godot) { Write-Error "Godot was not found. Pass its path with -Godot." }

# One lap by the computer, recorded; returns the out lap's time in seconds
# (where the timed lap starts in the replay) and prints the run.
function Invoke-Lap([string[]]$lapArgs, [string]$file) {
    $text = & $sim ai @lapArgs --laps 1 --replay $file
    $text | ForEach-Object { Write-Host "  $_" }
    $line = $text | Where-Object { $_ -match '^out lap\s+(\d+):(\d+\.\d+)' } | Select-Object -First 1
    if (-not $line) { Write-Error "No out lap in the run for $file." }
    $null = $line -match '^out lap\s+(\d+):(\d+\.\d+)'
    return [double]$Matches[1] * 60 + [double]$Matches[2]
}

Write-Host "Ridgeline hot lap: Tern GT, grip, pace 1.0"
$ridge = Join-Path $replays "ridgeline.rpl"
$ridgeFrom = Invoke-Lap @("--car", "hatch_tern_gt", "--track", "ridgeline_circuit", "--pace", "1.0") $ridge
Write-Host "Harrow tandem: the follower, and the lead started 30 m ahead (as the game's tandem mode)"
$follow = Join-Path $replays "harrow_follow.rpl"
$lead = Join-Path $replays "harrow_lead.rpl"
$harrowFrom = Invoke-Lap @("--car", "coupe_kestrel_rc", "--track", "harrow_drift_park", "--mode", "drift", "--pace", "0.9") $follow
$null = Invoke-Lap @("--car", "coupe_kestrel_rc", "--track", "harrow_drift_park", "--mode", "drift", "--pace", "0.9", "--ahead", "30") $lead
# Into the lap: the linked T1 to T3 and the T4 hairpin.
$harrowFrom += 8.0

$userArgs = @(
    "--steam-kit=$Out",
    "--ridgeline=$ridge", ("--ridgeline-from={0:F3}" -f $ridgeFrom), "--ridgeline-seconds=18",
    "--harrow=$follow", "--harrow-lead=$lead", ("--harrow-from={0:F3}" -f $harrowFrom), "--harrow-seconds=20",
    "--kit-step=$Step"
)
if ($StillsOnly) { $userArgs += "--kit-stills-only=1" }
if ($Preset) { $userArgs += "--kit-preset=$Preset" }
Write-Host "Rendering (the window shows the scene; it takes a few minutes)"
$log = Join-Path $Out "render.log"
# -OpenGL renders with the Compatibility renderer, as the Very Low preset
# runs (D-136).
$engineArgs = @("--path", (Join-Path $root "godot"))
if ($OpenGL) { $engineArgs += @("--rendering-method", "gl_compatibility") }
$p = Start-Process $Godot -ArgumentList ($engineArgs + @("--") + $userArgs) -Wait -PassThru -RedirectStandardOutput $log -RedirectStandardError "$log.err"
Get-Content $log | Where-Object { $_ -match "steam kit" } | ForEach-Object { Write-Host "  $_" }
if ($p.ExitCode -ne 0) {
    Get-Content "$log.err" | Select-Object -Last 20 | ForEach-Object { Write-Host $_ }
    Write-Error "Rendering failed with exit code $($p.ExitCode)."
}

if ($StillsOnly) {
    Get-ChildItem (Join-Path $Out "screenshots") -Filter *.png | ForEach-Object { Write-Host ("  {0}  {1:N1} MB" -f $_.Name, ($_.Length / 1MB)) }
    exit 0
}
$rate = [math]::Max([int](60 / $Step), 1)
$video = Join-Path $Out "trailer_1080p60.mp4"
& $Ffmpeg -hide_banner -loglevel error -y -framerate $rate -i (Join-Path $Out "trailer\frame_%05d.png") -r 60 -c:v libx264 -preset slow -crf 16 -pix_fmt yuv420p -movflags +faststart $video
if ($LASTEXITCODE -ne 0) { Write-Error "ffmpeg failed." }
Write-Host ""
Write-Host "Screenshots:"
Get-ChildItem (Join-Path $Out "screenshots") -Filter *.png | ForEach-Object { Write-Host ("  {0}  {1:N1} MB" -f $_.Name, ($_.Length / 1MB)) }
Write-Host ("Trailer: {0}  {1:N1} MB" -f $video, ((Get-Item $video).Length / 1MB))
