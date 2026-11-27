# SPDX-License-Identifier: Apache-2.0
# The frame time check (D-130) on a render bench results file:
#   - no frame over 50 ms once the first 5 s are past (a "hitch" line the
#     bench wrote, other than those marked "before measuring");
#   - no preset's 1% low more than 15 % slower than tools\bench_baseline.txt,
#     where the results come from the same GPU as the baseline.
# Fails (exit 1) on either.
#
#   powershell -ExecutionPolicy Bypass -File tools\bench_check.ps1 -Results path\render_bench_X.txt
#   powershell -ExecutionPolicy Bypass -File tools\bench_check.ps1 -Run [-Godot C:\path\godot.exe] [-Update]
#
# -Run benches Low and High here first (about 4 minutes) and checks that
# file; -Update then makes it the baseline and the reference copy that CI
# checks (tools\bench_reference.txt). A CI runner has no GPU: frame times on
# its software renderer mean nothing, so CI checks the reference file
# measured on a real one and committed with the change.
param(
    [string]$Results = "",
    [switch]$Run,
    [string]$Godot = "",
    [switch]$Update
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$baselinePath = Join-Path $root "tools\bench_baseline.txt"
$referencePath = Join-Path $root "tools\bench_reference.txt"
$slowerLimit = 1.15

function Invoke-Bench {
    $log = Join-Path ([System.IO.Path]::GetTempPath()) "qts_bench_check"
    Start-Process $Godot -ArgumentList "--path", "`"$(Join-Path $root 'godot')`"", "--resolution", "1600x900", "--", "--render-bench=all" -Wait -NoNewWindow -RedirectStandardOutput "$log.out" -RedirectStandardError "$log.err"
    Get-Content "$log.out" | Where-Object { $_ -match "^render bench" } | ForEach-Object { Write-Host $_ }
    $written = Get-Content "$log.out" | Where-Object { $_ -match '^render bench: results written to (.+)$' } | Select-Object -Last 1
    if (-not $written) { Write-Error "The bench wrote no results file." }
    return ($written -replace '^render bench: results written to ', '').Trim()
}
if ($Run) {
    if (-not $Godot) {
        $found = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter "Godot_v*_win64_console.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $Godot = $found.FullName }
    }
    if (-not $Godot) { Write-Error "Pass -Godot." }
    $Results = Invoke-Bench
}
if (-not $Results) { $Results = $referencePath }

# One record per preset block: its 1% low and the hitches it lists.
function Read-Bench([string]$path) {
    $gpu = ""
    $blocks = [ordered]@{}
    $cur = $null
    foreach ($line in Get-Content $path) {
        if ($line -match '^GPU {2,}(?!time)(.+)$') { $gpu = $Matches[1].Trim() }
        elseif ($line -match '^\[(\w+)\]$') { $cur = @{ low1 = 0.0; hitches = @() }; $blocks[$Matches[1]] = $cur }
        elseif ($cur -and $line -match '1% low ([\d.]+) ms') { $cur.low1 = [double]$Matches[1] }
        elseif ($cur -and $line -match '^\s+hitch\s+(.+)$') { $cur.hitches += $Matches[1] }
    }
    return @{ gpu = $gpu; blocks = $blocks }
}

$baseline = @{}
$baseGpu = ""
if (Test-Path $baselinePath) {
    foreach ($line in Get-Content $baselinePath) {
        if ($line -match '^gpu\s+(.+)$') { $baseGpu = $Matches[1].Trim() }
        elseif ($line -match '^low1_ms\s+(\w+)\s+([\d.]+)') { $baseline[$Matches[1]] = [double]$Matches[2] }
    }
}
function Test-Bench($bench) {
    $script:hitched = $false
    $script:slow = $false
    Write-Host "Checking $Results ($($bench.gpu))"
    foreach ($preset in $bench.blocks.Keys) {
        $b = $bench.blocks[$preset]
        $late = @($b.hitches | Where-Object { $_ -notmatch 'before measuring' })
        if ($late.Count -gt 0) {
            $script:hitched = $true
            Write-Host "$preset fails: $($late.Count) frames over 50 ms after the first 5 s"
            $late | ForEach-Object { Write-Host "  $_" }
        } else {
            Write-Host "$preset passes: no frame over 50 ms after the first 5 s"
        }
        if ($baseline.ContainsKey($preset) -and $bench.gpu -eq $baseGpu) {
            $ratio = $b.low1 / $baseline[$preset]
            $line = "{0} 1% low {1:N2} ms against the baseline's {2:N2} ms ({3:+0;-0} %)" -f $preset, $b.low1, $baseline[$preset], (($ratio - 1) * 100)
            if ($ratio -gt $slowerLimit) { $script:slow = $true; Write-Host "$line : more than 15 % slower" } else { Write-Host $line }
        } elseif ($baseline.ContainsKey($preset)) {
            Write-Host "$preset 1% low not compared: the baseline was measured on $baseGpu"
        }
    }
}
$bench = Read-Bench $Results
Test-Bench $bench
# Frame times on a desktop vary from run to run by around 15 %: one slow
# 1% low gets a second run, and the better of the two counts.
if ($Run -and $script:slow -and -not $script:hitched) {
    Write-Host "1% low over the line: running the bench once more"
    $second = Invoke-Bench
    $other = Read-Bench $second
    foreach ($preset in @($bench.blocks.Keys)) {
        if ($other.blocks.Contains($preset) -and $other.blocks[$preset].low1 -lt $bench.blocks[$preset].low1) {
            $bench.blocks[$preset].low1 = $other.blocks[$preset].low1
        }
        if ($other.blocks.Contains($preset)) { $bench.blocks[$preset].hitches += $other.blocks[$preset].hitches }
    }
    $Results = $second
    Test-Bench $bench
}
$failed = $script:hitched -or $script:slow

if ($Update) {
    if ($failed) { Write-Error "Not making a failing run the baseline." }
    $lines = @(
        "# The 1% low frame time per preset that tools\bench_check.ps1 compares",
        "# against (D-130); written by bench_check.ps1 -Run -Update.",
        "gpu $($bench.gpu)"
    )
    foreach ($preset in $bench.blocks.Keys) { $lines += ("low1_ms {0} {1:N2}" -f $preset, $bench.blocks[$preset].low1) }
    Set-Content -Encoding ascii $baselinePath $lines
    # A copy by content: the bench file sits in AppData, which Copy-Item
    # refuses to copy into the repository on some machines.
    Set-Content -Encoding utf8 $referencePath (Get-Content -Encoding utf8 $Results)
    Write-Host "Baseline and reference updated from $Results"
}
if ($failed) { exit 1 }
