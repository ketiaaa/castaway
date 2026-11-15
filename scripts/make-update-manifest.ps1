<#
.SYNOPSIS
  Writes latest.json, the file Castaway's in-app updater reads from the latest GitHub release.
.EXAMPLE
  scripts\make-update-manifest.ps1 -Version 0.1.2 -Notes "Fixes X" -OutDir C:\temp\rel
  Run after `npm run tauri build` with the updater key set (see docs/releasing.md).
#>
param(
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $Notes,
  [Parameter(Mandatory)] [string] $OutDir,
  [string] $Repo = 'ketiaaa/castaway'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$bundle = Join-Path $root 'src-tauri\target\release\bundle\nsis'
$exeName = "Castaway_${Version}_x64-setup.exe"
$exe = Join-Path $bundle $exeName
$sig = "$exe.sig"
if (-not (Test-Path $exe)) { throw "Missing $exe - run the build first." }
if (-not (Test-Path $sig)) { throw "Missing $sig - was the build run with the signing key set?" }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$manifest = [ordered]@{
  version  = $Version
  notes    = $Notes
  pub_date = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
  platforms = [ordered]@{
    'windows-x86_64' = [ordered]@{
      signature = (Get-Content $sig -Raw).Trim()
      url       = "https://github.com/$Repo/releases/download/v$Version/$exeName"
    }
  }
}
$json = $manifest | ConvertTo-Json -Depth 5
[System.IO.File]::WriteAllText((Join-Path $OutDir 'latest.json'), $json, (New-Object System.Text.UTF8Encoding $false))
Copy-Item $exe $OutDir -Force
Copy-Item $sig $OutDir -Force
$hash = (Get-FileHash $exe -Algorithm SHA256).Hash.ToLower()
[System.IO.File]::WriteAllText((Join-Path $OutDir 'SHA256SUMS.txt'), "$hash *$exeName`n", (New-Object System.Text.UTF8Encoding $false))
Copy-Item (Join-Path $root 'src-tauri\resources\THIRD-PARTY-NOTICES.txt') $OutDir -Force
Write-Host "Wrote release files to $OutDir"
