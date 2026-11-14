<#
Builds UxPlay (pinned tag) from source under MSYS2 UCRT64 WITHOUT Bonjour
(no -DUSE_DNS_SD=1) and vendors uxplay.exe + its runtime into
src-tauri/resources/uxplay/. Output is git-ignored except VERSION.txt.

Usage:  powershell -ExecutionPolicy Bypass -File scripts/build-uxplay.ps1 [-Msys2 C:\msys64]
#>
param([string]$Msys2 = 'C:\msys64')

$ErrorActionPreference = 'Stop'

# Pinned UxPlay release tag (https://github.com/FDH2/UxPlay/tags). Change here only.
$UxPlayTag = 'v1.73.7'

$Root   = Split-Path -Parent $PSScriptRoot
$OutDir = Join-Path $Root 'src-tauri\resources\uxplay'
$Work   = Join-Path $Root 'build\uxplay'   # git-ignored scratch dir

$bash = Join-Path $Msys2 'usr\bin\bash.exe'
if (-not (Test-Path $bash)) { throw "MSYS2 not found at $Msys2 (install from https://www.msys2.org)" }

function To-MsysPath([string]$p) {
    $p = (Resolve-Path -LiteralPath $p).Path.Replace('\', '/')
    return '/' + $p.Substring(0, 1).ToLower() + $p.Substring(2)
}

New-Item -ItemType Directory -Force $Work, $OutDir | Out-Null
$env:MSYSTEM = 'UCRT64'
$env:CHERE_INVOKING = '1'
$env:UXPLAY_TAG = $UxPlayTag
$env:UXPLAY_WORK = To-MsysPath $Work
$env:UXPLAY_OUT = To-MsysPath $OutDir

& $bash -lc "$(To-MsysPath (Join-Path $PSScriptRoot 'build-uxplay.sh'))"
if ($LASTEXITCODE -ne 0) { throw "build-uxplay.sh failed ($LASTEXITCODE)" }

# BOM-less, no trailing newline needed
[System.IO.File]::WriteAllText((Join-Path $OutDir 'VERSION.txt'), "$UxPlayTag`n")
Write-Host "UxPlay $UxPlayTag vendored into $OutDir"
