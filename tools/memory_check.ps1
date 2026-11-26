# SPDX-License-Identifier: Apache-2.0
# The memory budget check (D-128): Harrow on High, the computer driving for
# a minute, and the RAM and video memory it took against
# tools\memory_budget.txt. Fails (exit 1) when either is over.
#
#   powershell -ExecutionPolicy Bypass -File tools\memory_check.ps1 -Exe out\QuickTurnSimDemo\QuickTurnSimDemo.exe
#   powershell -ExecutionPolicy Bypass -File tools\memory_check.ps1 -Godot C:\path\to\godot.exe   (from the repository)
#
# -Software renders on SwiftShader, the software Vulkan driver that ships
# with Google Chrome, for a machine with no GPU (a CI runner). Its video
# memory lives in the process's RAM, so the RAM compared is the working set
# less Godot's own video memory count, and the video memory compared is
# Godot's count (the "ram" and "vram" lines). On a real GPU the working set
# and Windows' dedicated video memory count are compared with the "ram_gpu"
# and "vram_gpu" lines, which allow for the driver's own share.
param(
    [string]$Exe = "",
    [string]$Godot = "",
    [switch]$Software,
    [int]$Seconds = 60
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

$limits = @{}
foreach ($line in Get-Content (Join-Path $root "tools\memory_budget.txt")) {
    if ($line -match '^\s*(ram|vram|ram_gpu|vram_gpu)\s+([\d.]+)') { $limits[$Matches[1]] = [double]$Matches[2] }
}

if ($Software) {
    $icd = Get-ChildItem "${env:ProgramFiles}\Google\Chrome\Application", "${env:ProgramFiles(x86)}\Google\Chrome\Application" -Recurse -Filter "vk_swiftshader_icd.json" -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $icd) { Write-Error "SwiftShader was not found: it ships with Google Chrome." }
    $env:VK_ICD_FILENAMES = $icd.FullName
    $env:VK_DRIVER_FILES = $icd.FullName
    Write-Host "Rendering on SwiftShader: $($icd.FullName)"
}

$userArgs = @("--memory-check", "--bench-seconds=$Seconds", "--bench-3d-height=1080")
if ($Exe) {
    $run = $Exe
    $runArgs = @("--resolution", "1920x1080", "--") + $userArgs
} else {
    if (-not $Godot) {
        $found = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter "Godot_v*_win64_console.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $Godot = $found.FullName }
    }
    if (-not $Godot) { Write-Error "Pass -Exe (a packaged build) or -Godot." }
    $run = $Godot
    $runArgs = @("--path", (Join-Path $root "godot"), "--resolution", "1920x1080", "--") + $userArgs
}
# The Vulkan loader that comes with Chrome, beside the game for this run:
# Windows looks there first, and a CI runner's own loader (if it has one)
# finds no driver and offers no window surface.
$loaderCopy = ""
if ($Software) {
    $beside = Join-Path (Split-Path -Parent (Resolve-Path $run)) "vulkan-1.dll"
    if (-not (Test-Path $beside)) {
        Copy-Item (Join-Path $icd.DirectoryName "vulkan-1.dll") $beside
        $loaderCopy = $beside
    }
}

# A packaged game is a windowed program: PowerShell neither waits for it nor
# sees its output unless it is started this way.
$log = Join-Path ([System.IO.Path]::GetTempPath()) "qts_memory_check"
$quoted = $runArgs | ForEach-Object { if ($_ -match ' ') { "`"$_`"" } else { $_ } }
$p = Start-Process $run -ArgumentList $quoted -PassThru -RedirectStandardOutput "$log.out" -RedirectStandardError "$log.err"
# A software renderer is slow, but not this slow: past the limit the game
# is stuck (an error box waiting for a click, say). Stop it and show why.
$limitMs = ($Seconds + 540) * 1000
$finished = $p.WaitForExit($limitMs)
if (-not $finished) { Stop-Process -Id $p.Id -Force }
$p.WaitForExit()
if ($loaderCopy) { Remove-Item -Force -Confirm:$false $loaderCopy -ErrorAction SilentlyContinue }
if (-not $finished) {
    Get-Content "$log.out", "$log.err" -ErrorAction SilentlyContinue | Select-Object -Last 40 | ForEach-Object { Write-Host $_ }
    Write-Error "The game did not finish within $($limitMs / 1000) s."
}
$out = @(Get-Content "$log.out", "$log.err" -ErrorAction SilentlyContinue)
if ($p.ExitCode -ne 0) { $out | Select-Object -Last 20 | ForEach-Object { Write-Host $_ } }
$out | Where-Object { $_ -match "memory|render bench|Vulkan" } | ForEach-Object { Write-Host $_ }
$result = $out | Where-Object { $_ -match '^memory check: ' } | Select-Object -Last 1
if (-not $result) { Write-Error "The game printed no memory check line." }
$v = @{}
foreach ($pair in ($result -replace '^memory check: ', '') -split ' ') {
    $kv = $pair -split '=', 2
    if ($kv.Count -eq 2) { $v[$kv[0]] = $kv[1] }
}
$software = $v["software_gpu"] -eq "true"
$checks = if ($software) {
    @(@("ram", [double]$v["ram_excl_video_mb"]), @("vram", [double]$v["vram_godot_mb"]))
} else {
    @(@("ram_gpu", [double]$v["ram_mb"]), @("vram_gpu", [double]$v["vram_driver_mb"]))
}
$failed = $false
foreach ($c in $checks) {
    $limit = $limits[$c[0]]
    if ($c[1] -gt $limit) {
        Write-Host ("Over the {0} budget: {1:N0} MB, budget {2} MB." -f $c[0], $c[1], $limit)
        $failed = $true
    } else {
        Write-Host ("Within the {0} budget: {1:N0} MB of {2} MB." -f $c[0], $c[1], $limit)
    }
}
if ($failed) { exit 1 }
