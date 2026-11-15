# SPDX-License-Identifier: Apache-2.0
# Independent evaluation of the Magic Formula subset used by qts-tire.
# The reference numbers in crates/qts-tire/tests/magic_formula.rs were produced
# by this script. It shares no code with the Rust implementation, so agreement
# between the two guards against transcription errors in either.
# Run: powershell -File tools/mf_reference.ps1

function MF([double]$B, [double]$C, [double]$D, [double]$E, [double]$x) {
    $bx = $B * $x
    return $D * [math]::Sin($C * [math]::Atan($bx - $E * ($bx - [math]::Atan($bx))))
}
function MFcos([double]$B, [double]$C, [double]$D, [double]$E, [double]$x) {
    $bx = $B * $x
    return $D * [math]::Cos($C * [math]::Atan($bx - $E * ($bx - [math]::Atan($bx))))
}

"# basic curve, B=10 C=1.9 D=1 E=0.97"
foreach ($x in 0.05, 0.1, 0.2, 0.5, 1.0) {
    "{0} {1:R}" -f $x, (MF 10 1.9 1 0.97 $x)
}

# Coefficients of the test tire in qts_tire::test_support::tire().
$Fz0 = 4000.0; $R0 = 0.31
$pcx1 = 1.6; $pdx1 = 1.1; $pdx2 = -0.1; $pex1 = 0.2; $pex2 = 0.0
$pkx1 = 20.0; $pkx2 = 0.0; $pkx3 = 0.2
$pcy1 = 1.4; $pdy1 = 1.0; $pdy2 = -0.15; $pdy3 = 2.0; $pey1 = -0.5; $pey2 = 0.0
$pky1 = 18.0; $pky2 = 2.0; $pky3 = 0.3; $pvy3 = 1.0
$rbx1 = 12.0; $rbx2 = 10.0; $rcx1 = 1.0; $rby1 = 10.0; $rby2 = 8.0; $rby3 = 0.0; $rcy1 = 1.0
$qbz1 = 10.0; $qbz2 = -1.0; $qcz1 = 1.1; $qdz1 = 0.1; $qdz2 = -0.01; $qez1 = -1.5
$ppx1 = -0.3; $ppx3 = -0.1; $ppx4 = 0.0; $ppy1 = 0.4; $ppy2 = 1.3; $ppy3 = -0.1; $ppy4 = 0.0

function Tire([double]$Fz, [double]$kappa, [double]$alphaDeg, [double]$camberDeg, [double]$dpi) {
    $ta = [math]::Tan($alphaDeg * [math]::PI / 180.0)
    $g = $camberDeg * [math]::PI / 180.0
    $dfz = ($Fz - $Fz0) / $Fz0
    $Kx = $Fz * ($pkx1 + $pkx2 * $dfz) * [math]::Exp($pkx3 * $dfz) * (1 + $ppx1 * $dpi)
    $Ky = $pky1 * $Fz0 * (1 + $ppy1 * $dpi) * [math]::Sin(2 * [math]::Atan($Fz / ($pky2 * (1 + $ppy2 * $dpi) * $Fz0))) * (1 - $pky3 * [math]::Abs($g))
    $mux = ($pdx1 + $pdx2 * $dfz) * (1 + $ppx3 * $dpi + $ppx4 * $dpi * $dpi)
    $muy = ($pdy1 + $pdy2 * $dfz) * (1 + $ppy3 * $dpi + $ppy4 * $dpi * $dpi) * (1 - $pdy3 * $g * $g)
    $Dx = $mux * $Fz; $Ex = [math]::Min($pex1 + $pex2 * $dfz, 1.0); $Bx = $Kx / ($pcx1 * $Dx)
    $Fx0 = MF $Bx $pcx1 $Dx $Ex $kappa
    $Dy = $muy * $Fz; $Ey = [math]::Min($pey1 + $pey2 * $dfz, 1.0); $By = $Ky / ($pcy1 * $Dy)
    $Fy0 = MF $By $pcy1 $Dy $Ey $ta
    $Sv = $Fz * $pvy3 * $g
    $Bxa = $rbx1 * [math]::Cos([math]::Atan($rbx2 * $kappa))
    $Gxa = [math]::Cos($rcx1 * [math]::Atan($Bxa * $ta))
    $Byk = $rby1 * [math]::Cos([math]::Atan($rby2 * ($ta - $rby3)))
    $Gyk = [math]::Cos($rcy1 * [math]::Atan($Byk * $kappa))
    $Fx = $Gxa * $Fx0
    $FySlip = $Gyk * $Fy0
    $Fy = $FySlip + $Gyk * $Sv
    $sgn = 1.0; if ($ta -lt 0) { $sgn = -1.0 }
    $eq = [math]::Sqrt($ta * $ta + ($Kx / $Ky) * ($Kx / $Ky) * $kappa * $kappa) * $sgn
    $Bt = $qbz1 + $qbz2 * $dfz
    $Dt = $Fz * ($qdz1 + $qdz2 * $dfz) * ($R0 / $Fz0)
    $t = (MFcos $Bt $qcz1 $Dt $qez1 $eq) / [math]::Sqrt(1 + $ta * $ta)
    $Mz = -$t * $FySlip
    "{0} {1} {2} {3} {4} => Fx={5:R} Fy={6:R} Mz={7:R} Kx={8:R} Ky={9:R}" -f $Fz, $kappa, $alphaDeg, $camberDeg, $dpi, $Fx, $Fy, $Mz, $Kx, $Ky
}

"# full model: Fz kappa alpha_deg camber_deg dpi"
Tire 4000 0.1 0 0 0
Tire 6000 0 4 0 0
Tire 3000 -0.05 -6 2 0
Tire 5000 0.08 3 0 0.1
Tire 2000 0.3 12 -1.5 -0.15
