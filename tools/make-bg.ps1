# Generate a visual-novel background (minimal-line style).
#
# ASCII only on purpose: Windows PowerShell 5.1 reads .ps1 files as the system
# ANSI codepage, so non-ASCII comments break parsing on non-UTF8-BOM files.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File tools/make-bg.ps1 `
#       -Name cell -Top '#242430' -Bottom '#15151d' -LineWidth 5 `
#       -Lines 'line 120,0 120,470 line 340,0 340,470'
#
#   # same place, different light: change colors only
#   powershell -ExecutionPolicy Bypass -File tools/make-bg.ps1 `
#       -Name cell_night -Top '#161a2e' -Bottom '#0c0e18' -LineWidth 5 `
#       -Lines 'line 120,0 120,470' `
#       -Keys 'fill #d8dce8 stroke none circle 980,150 980,60'
#
#   # add a shaft of light
#   powershell -ExecutionPolicy Bypass -File tools/make-bg.ps1 `
#       -Name cell_dawn -Top '#2a2c3c' -Bottom '#181a26' `
#       -Lines 'line 120,0 120,470' `
#       -Light '560,0 780,0 900,470 680,470' -LightColor '#666c80' -LightBlur 30
#
# Rules and palette: docs/art-direction.md
#
# -Name       asset name; writes <Project>/assets/bg/<Name>.png
# -Top        gradient top color (keep it dark, see art-direction)
# -Bottom     gradient bottom color
# -Lines      stroke-only shapes: structure (bars, benches, road edges)
# -Keys       filled shapes: "what is there" (moon, crowd, windows)
# -Light      one polygon used as a soft light shaft, coords "x1,y1 x2,y2 ..."
# -LineWidth  stroke width. THIS IS AN EMOTION PARAMETER, not a technical one:
#             2-3 cold/empty/distant, 5 restrained, 7-9 heavy/authority
# -Chroma     color count before writing (default 256, shrinks the file)
# -NoOptimize skip color reduction (for debugging)

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Name,
    [Parameter(Mandatory = $true)][string]$Top,
    [Parameter(Mandatory = $true)][string]$Bottom,
    [string]$Lines = '',
    [string]$Keys = '',
    [string]$Light = '',
    [string]$LightColor = '#666c80',
    [int]$LightBlur = 30,
    [int]$LineWidth = 5,
    [string]$LineColor = '#3a3a4a',
    [int]$Chroma = 128,
    [string]$Project = '',
    [string]$Magick = '',
    [int]$JpegQuality = 0,
    [switch]$Jpeg,
    [switch]$NoOptimize
)

$ErrorActionPreference = 'Stop'

# --- locate ImageMagick -------------------------------------------
if (-not $Magick) {
    $cmd = Get-Command magick -ErrorAction SilentlyContinue
    if ($cmd) { $Magick = $cmd.Source }
}
if (-not $Magick) {
    $found = Get-ChildItem "$env:ProgramFiles" -Directory -Filter 'ImageMagick*' -ErrorAction SilentlyContinue |
        Sort-Object Name -Descending
    foreach ($d in $found) {
        $exe = Join-Path $d.FullName 'magick.exe'
        if (Test-Path $exe) { $Magick = $exe; break }
    }
}
if (-not $Magick) { throw 'magick.exe not found. Install ImageMagick 7 or pass -Magick <path>.' }

# --- locate the project -------------------------------------------
if ($Project) {
    $bgDir = Join-Path $Project 'assets\bg'
}
else {
    # walk up from this script looking for a sibling "examples" directory
    $dir = $PSScriptRoot
    $bgDir = $null
    for ($i = 0; $i -lt 4 -and $dir; $i++) {
        $examples = Join-Path $dir 'examples'
        if (Test-Path $examples) {
            $first = Get-ChildItem $examples -Directory -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($first) { $bgDir = Join-Path $first.FullName 'assets\bg'; break }
        }
        $dir = Split-Path -Parent $dir
    }
    if (-not $bgDir) { throw 'Could not find a project. Pass -Project <dir>.' }
}
if (-not (Test-Path $bgDir)) { New-Item -ItemType Directory -Path $bgDir -Force | Out-Null }
$out = Join-Path $bgDir "$Name.png"

$W = 1280
$H = 720
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) "tsn_bg_$Name"
if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force }
New-Item -ItemType Directory -Path $tmp -Force | Out-Null

# --- 1. base gradient --------------------------------------------
$cur = Join-Path $tmp 'cur.png'
& $Magick -size "${W}x${H}" "gradient:$Top-$Bottom" $cur
if ($LASTEXITCODE -ne 0) { throw 'gradient failed' }

# --- 2. keys (filled) then lines (stroked) -----------------------
# Order matters: filled shapes say "what is there", strokes are structure
# drawn on top. Swap them if you want the structure behind.
if ($Keys) {
    $next = Join-Path $tmp 'keys.png'
    & $Magick $cur -draw $Keys $next
    if ($LASTEXITCODE -ne 0) { throw "-Keys draw failed: $Keys" }
    $cur = $next
}
if ($Lines) {
    $next = Join-Path $tmp 'lines.png'
    & $Magick $cur -draw "fill none stroke $LineColor stroke-width $LineWidth $Lines" $next
    if ($LASTEXITCODE -ne 0) { throw "-Lines draw failed: $Lines" }
    $cur = $next
}

# --- 3. light ----------------------------------------------------
# Brighter light = stronger meaning, but it must not swallow the structure.
# See docs/art-direction.md (weak beats strong).
if ($Light) {
    $glow = Join-Path $tmp 'glow.png'
    $next = Join-Path $tmp 'lit.png'
    & $Magick -size "${W}x${H}" xc:none -fill $LightColor -draw "polygon $Light" -blur "0x$LightBlur" $glow
    if ($LASTEXITCODE -ne 0) { throw '-Light draw failed' }
    & $Magick $cur $glow -compose over -composite $next
    $cur = $next
}

# --- 4. write ----------------------------------------------------
# PNG8 (8-bit palette) is what actually shrinks these files, and turning
# DITHERING OFF makes it both smaller and cleaner. Measured on a 1280x720
# image with a light shaft:
#     plain 24-bit PNG                   647,664 B
#     PNG8 + colors=128, dithered         43,914 B   (visible noise in gradients)
#     PNG8 + colors=128, NO dither        15,164 B   <- default
#     PNG8 + Floyd-Steinberg             126,354 B
# A gentle dark gradient fits in 128 colors without banding, so the dither
# pattern is pure noise. Raise -Chroma if you ever see banding.
# JPEG is smaller still but smears the hard line edges, so it is opt-in.
if ($NoOptimize) {
    Copy-Item $cur $out -Force
}
elseif ($Jpeg) {
    $jpg = [System.IO.Path]::ChangeExtension($out, '.jpg')
    $q = if ($JpegQuality -gt 0) { $JpegQuality } else { 90 }
    & $Magick $cur -quality $q -strip $jpg
    if ($LASTEXITCODE -ne 0) { throw 'write failed' }
    if (Test-Path $out) { Remove-Item $out -Force }
    $out = $jpg
}
else {
    & $Magick $cur +dither -colors $Chroma -define png:compression-level=9 -strip "PNG8:$out"
    if ($LASTEXITCODE -ne 0) { throw 'write failed' }
}

Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue

$size = (Get-Item $out).Length
$lit = if ($Light) { $LightColor } else { 'none' }
Write-Host ("  {0,-22} {1,9:N0} B   line={2}  light={3}" -f "$Name.png", $size, $LineWidth, $lit)
Write-Host "  -> $out"
