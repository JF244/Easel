<#
.SYNOPSIS
  Looks at this Windows PC and prints Easel's setup profile as JSON. Changes nothing.

.DESCRIPTION
  Detects the graphics card (and its real memory and NVIDIA compute capability),
  RAM, CPU, free disk, which local model servers are running, and which tools are
  installed; then resolves that into what Easel's setup would install: the compute
  backend (CUDA, legacy CUDA for Pascal, Gemini for AMD on Windows, CPU), ComfyUI
  version and PyTorch build, checkpoint, local brain model and context window,
  and warnings. Run it on a PC first to see what the installer will do there.

.PARAMETER Save
  Also save the profile to %LOCALAPPDATA%\EaselSuite\hardware-profile.json, where
  Easel reads it to warn before ComfyUI jobs this GPU can't run.

.PARAMETER Test
  Run the shared resolver test cases instead.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File detect-hardware.ps1
  powershell -ExecutionPolicy Bypass -File detect-hardware.ps1 -Save > profile.json
#>
[CmdletBinding()]
param([switch]$Save, [switch]$Test, [string]$InstallDir = $env:LOCALAPPDATA)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
. (Join-Path $PSScriptRoot "EaselProfile.ps1")

if ($Test) {
  if (Invoke-EaselProfileCases) { exit 0 } else { exit 1 }
}

$facts = Get-EaselHardwareFacts $InstallDir
$resolved = Resolve-EaselProfile $facts
$hw = [pscustomobject][ordered]@{ detected = $facts; resolved = $resolved; summary = $resolved.summary }
$json = $hw | ConvertTo-Json -Depth 20

if ($Save) {
  $dir = Join-Path $env:LOCALAPPDATA "EaselSuite"
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  $path = Join-Path $dir "hardware-profile.json"
  [IO.File]::WriteAllText($path, $json, (New-Object Text.UTF8Encoding($false)))
  Write-Host "Saved $path" -ForegroundColor Green
}
$json
