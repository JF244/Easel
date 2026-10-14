<#
.SYNOPSIS
  Sets up Easel and everything around it on a Windows PC, sized to its hardware.

.DESCRIPTION
  Looks at this PC (graphics card and its memory, RAM, free disk space, what's
  already installed), shows a plan, and after you say yes:

    - installs Easel (from the installer next to this script)
    - installs the Crafting Apps: PhotoCraft, VectorCraft, FilmCraft,
      LightCraft, PdfCraft, EffectCraft, DesignCraft
    - installs ComfyUI (NVIDIA, AMD or CPU build) and an image model that fits
      your graphics memory, with a Start ComfyUI shortcut
    - installs Ollama and a local brain model that fits your PC
    - installs Git for Windows and Claude Code (the brain that can drive the editors)
    - points Easel's settings at all of it

  Running it again is safe: anything already installed is skipped.

.PARAMETER DryRun
  Only detect and show the plan. Changes nothing.

.PARAMETER CheckLinks
  With -DryRun: also check that every download in the plan is reachable.

.PARAMETER Yes
  Don't ask; accept the plan.

.EXAMPLE
  .\Easel-Setup.ps1
  .\Easel-Setup.ps1 -DryRun -CheckLinks
  .\Easel-Setup.ps1 -SkipModels -InstallDir D:\Easel
#>
[CmdletBinding()]
param(
  [switch]$DryRun,
  [switch]$CheckLinks,
  [switch]$Yes,
  [string]$InstallDir = "",
  [switch]$SkipApps,
  [switch]$SkipComfy,
  [switch]$SkipModels,
  [switch]$SkipLocalBrain,
  [switch]$SkipClaude,
  [switch]$NoAutostart,
  # Don't open ComfyUI, Claude Code's sign-in or Easel at the end (used by the automated test).
  [switch]$NoLaunch
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"  # Invoke-WebRequest is 10x slower with the progress bar on.
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# ---------------------------------------------------------------------------
# What gets installed, and from where. Release file names are looked up at run
# time from each project's latest GitHub release, so new versions just work.
# ---------------------------------------------------------------------------

$CraftApps = @(
  @{ Id = "photocraft";  Name = "PhotoCraft";  Replaces = "Photoshop" },
  @{ Id = "vectorcraft"; Name = "VectorCraft"; Replaces = "Illustrator" },
  @{ Id = "filmcraft";   Name = "FilmCraft";   Replaces = "Premiere Pro" },
  @{ Id = "lightcraft";  Name = "LightCraft";  Replaces = "Lightroom" },
  @{ Id = "pdfcraft";    Name = "PdfCraft";    Replaces = "Acrobat" },
  @{ Id = "effectcraft"; Name = "EffectCraft"; Replaces = "After Effects" },
  @{ Id = "designcraft"; Name = "DesignCraft"; Replaces = "InDesign" }
)

$ComfyRepo = "Comfy-Org/ComfyUI"
$SevenZipUrl = "https://www.7-zip.org/a/7zr.exe"
$OllamaSetupUrl = "https://ollama.com/download/OllamaSetup.exe"
$ClaudeInstallUrl = "https://claude.ai/install.ps1"

# Image models by graphics memory. All are openly downloadable, no account needed.
$ImageModels = @(
  @{ MinVramGB = 16; Name = "FLUX.1 schnell (fp8)"; File = "flux1-schnell-fp8.safetensors"; SizeGB = 17.2
     Url = "https://huggingface.co/Comfy-Org/flux1-schnell/resolve/main/flux1-schnell-fp8.safetensors"
     Steps = 4; Cfg = 1.0; Sampler = "euler"; Scheduler = "simple"; BaseSize = 1024; Negative = "" },
  @{ MinVramGB = 8; Name = "Stable Diffusion XL base"; File = "sd_xl_base_1.0.safetensors"; SizeGB = 6.9
     Url = "https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/resolve/main/sd_xl_base_1.0.safetensors"
     Steps = 28; Cfg = 6.0; Sampler = "euler"; Scheduler = "normal"; BaseSize = 1024; Negative = "blurry, low quality, watermark, text artifacts" },
  @{ MinVramGB = 4; Name = "Stable Diffusion 1.5"; File = "v1-5-pruned-emaonly.safetensors"; SizeGB = 4.3
     Url = "https://huggingface.co/stable-diffusion-v1-5/stable-diffusion-v1-5/resolve/main/v1-5-pruned-emaonly.safetensors"
     Steps = 25; Cfg = 7.0; Sampler = "euler"; Scheduler = "normal"; BaseSize = 512; Negative = "blurry, low quality, watermark, text artifacts" }
)

# Local brain models for Ollama, by graphics memory (or system RAM without a GPU).
$BrainModels = @(
  @{ MinGB = 16; Model = "gpt-oss:20b"; SizeGB = 14 },
  @{ MinGB = 10; Model = "qwen3:14b";  SizeGB = 9.3 },
  @{ MinGB = 6;  Model = "qwen3:8b";   SizeGB = 5.2 },
  @{ MinGB = 0;  Model = "qwen3:4b";   SizeGB = 2.6 }
)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA "EaselSuite" }
$LogDir = Join-Path $env:LOCALAPPDATA "EaselSuite"
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$LogFile = Join-Path $LogDir "setup.log"

function Say([string]$Text, [string]$Color = "Gray") {
  Write-Host $Text -ForegroundColor $Color
  Add-Content -Path $LogFile -Value ("{0}  {1}" -f (Get-Date -Format s), $Text)
}
function Step([string]$Text) { Say ""; Say "== $Text" "Cyan" }
function Ok([string]$Text) { Say "   OK  $Text" "Green" }
function Skip([string]$Text) { Say "   --  $Text" "DarkGray" }
function Warn([string]$Text) { Say "   !!  $Text" "Yellow" }

function Test-Admin {
  $id = [Security.Principal.WindowsIdentity]::GetCurrent()
  (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-LatestRelease([string]$Repo) {
  $headers = @{ "User-Agent" = "Easel-Setup"; "Accept" = "application/vnd.github+json" }
  Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers $headers -TimeoutSec 60
}

function Find-Asset($Release, [string[]]$Patterns) {
  foreach ($pattern in $Patterns) {
    $match = $Release.assets | Where-Object { $_.name -match $pattern } | Select-Object -First 1
    if ($match) { return $match }
  }
  return $null
}

function Test-Url([string]$Url) {
  try {
    $r = Invoke-WebRequest -Uri $Url -Method Head -UseBasicParsing -MaximumRedirection 10 -TimeoutSec 60
    return [int]$r.StatusCode -lt 400
  } catch {
    # Some hosts refuse HEAD; a 1-byte ranged GET proves the file is there.
    try {
      $r = Invoke-WebRequest -Uri $Url -Headers @{ Range = "bytes=0-0" } -UseBasicParsing -TimeoutSec 60
      return [int]$r.StatusCode -lt 400
    } catch { return $false }
  }
}

# Big files go through curl.exe (built into Windows 10 and 11): it resumes
# interrupted downloads and retries, which matters for 17 GB models.
function Get-File([string]$Url, [string]$Destination) {
  $dir = Split-Path -Parent $Destination
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  $partial = "$Destination.part"
  $curl = Join-Path $env:SystemRoot "System32\curl.exe"
  if (Test-Path $curl) {
    & $curl --fail --location --retry 5 --retry-delay 5 --continue-at - --progress-bar --output $partial $Url
    if ($LASTEXITCODE -ne 0) { throw "Download failed ($LASTEXITCODE): $Url" }
  } else {
    Invoke-WebRequest -Uri $Url -OutFile $partial -UseBasicParsing
  }
  Move-Item -Force $partial $Destination
}

function Install-Msi([string]$Path) {
  $p = Start-Process msiexec.exe -ArgumentList @("/i", "`"$Path`"", "/qn", "/norestart") -Wait -PassThru
  if ($p.ExitCode -ne 0 -and $p.ExitCode -ne 3010) { throw "Installer exited with code $($p.ExitCode): $Path" }
}

function Test-Command([string]$Name) { [bool](Get-Command $Name -ErrorAction SilentlyContinue) }

function Update-Path {
  $machine = [Environment]::GetEnvironmentVariable("Path", "Machine")
  $user = [Environment]::GetEnvironmentVariable("Path", "User")
  $env:Path = "$machine;$user"
}

function Install-Winget([string]$Id) {
  if (-not (Test-Command "winget")) { throw "winget isn't available. Install 'App Installer' from the Microsoft Store, then run this again." }
  & winget install --exact --id $Id --silent --accept-source-agreements --accept-package-agreements --disable-interactivity | Out-Host
  # 0 = installed; -1978335189 = already installed and up to date.
  if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335189) { throw "winget couldn't install $Id (code $LASTEXITCODE)." }
  Update-Path
}

function New-Shortcut([string]$Path, [string]$Target, [string]$Arguments, [string]$WorkingDir, [string]$Description) {
  $shell = New-Object -ComObject WScript.Shell
  $link = $shell.CreateShortcut($Path)
  $link.TargetPath = $Target
  $link.Arguments = $Arguments
  $link.WorkingDirectory = $WorkingDir
  $link.Description = $Description
  $link.Save()
}

function Write-JsonNoBom([string]$Path, $Object) {
  # Easel reads plain UTF-8; Windows PowerShell's UTF8 adds a BOM, so write it directly.
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path) | Out-Null
  $json = $Object | ConvertTo-Json -Depth 20
  [IO.File]::WriteAllText($Path, $json, (New-Object Text.UTF8Encoding($false)))
}

function Set-Prop($Object, [string]$Name, $Value) {
  if ($Object.PSObject.Properties[$Name]) { $Object.$Name = $Value } else { $Object | Add-Member -NotePropertyName $Name -NotePropertyValue $Value }
}

function Get-Prop($Object, [string]$Name) {
  if ($Object -and $Object.PSObject.Properties[$Name]) { return $Object.$Name }
  return $null
}

# ---------------------------------------------------------------------------
# Detect this PC
# ---------------------------------------------------------------------------

function Get-Gpus {
  $gpus = @()
  $classKey = "HKLM:\SYSTEM\ControlSet001\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}"
  # The registry has the real memory size; Win32_VideoController caps it at 4 GB.
  $registry = @{}
  Get-ChildItem $classKey -ErrorAction SilentlyContinue | ForEach-Object {
    $p = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
    if ($p -and $p.DriverDesc) {
      $bytes = $p."HardwareInformation.qwMemorySize"
      if (-not $bytes) { $bytes = $p."HardwareInformation.MemorySize" }
      if ($bytes -is [byte[]]) { $bytes = [BitConverter]::ToUInt64(($bytes + (New-Object byte[] 8))[0..7], 0) }
      if ($bytes) { $registry[$p.DriverDesc] = [double]$bytes }
    }
  }
  foreach ($vc in Get-CimInstance Win32_VideoController) {
    $name = [string]$vc.Name
    $bytes = $registry[$name]
    if (-not $bytes) { $bytes = [double]$vc.AdapterRAM }
    $vendor = "other"
    if ($name -match "NVIDIA") { $vendor = "nvidia" }
    elseif ($name -match "AMD|Radeon") { $vendor = "amd" }
    elseif ($name -match "Intel") { $vendor = "intel" }
    if ($name -match "Microsoft Basic|Remote Display|Hyper-V|Parsec|Virtual") { continue }
    $gpus += [pscustomobject]@{ Name = $name; Vendor = $vendor; VramGB = [math]::Round($bytes / 1GB, 1) }
  }
  # nvidia-smi knows NVIDIA memory exactly.
  if (Test-Command "nvidia-smi") {
    try {
      $lines = & nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits 2>$null
      foreach ($line in $lines) {
        $parts = $line -split ","
        $gpu = $gpus | Where-Object { $_.Name -eq $parts[0].Trim() } | Select-Object -First 1
        if ($gpu) { $gpu.VramGB = [math]::Round([double]$parts[1] / 1024, 1) }
      }
    } catch { }
  }
  return $gpus
}

function Test-AmdSupported([string]$Name) {
  # ComfyUI's AMD build supports RDNA 3, 3.5 and 4: Radeon RX 7000 and 9000,
  # and the Radeon 780M/880M/890M/8060S class of laptop chips.
  return ($Name -match "RX\s*(7\d{3}|9\d{3})" -or $Name -match "Radeon\s*(7[4-9]0M|8[0-9]0M|80[4-6]0S|AI)" -or $Name -match "Radeon PRO W7\d{3}")
}

function Get-EaselInstall {
  foreach ($candidate in @(
      (Join-Path $env:LOCALAPPDATA "Easel\easel.exe"),
      (Join-Path $env:LOCALAPPDATA "Programs\Easel\easel.exe"),
      (Join-Path $env:ProgramFiles "Easel\easel.exe"))) {
    if (Test-Path $candidate) { return $candidate }
  }
  return $null
}

function Get-CraftInstall([hashtable]$App) {
  foreach ($base in @($env:ProgramFiles, ${env:ProgramFiles(x86)}, (Join-Path $env:LOCALAPPDATA "Programs"))) {
    if (-not $base) { continue }
    $exe = Join-Path $base "$($App.Name)\$($App.Id).exe"
    if (Test-Path $exe) { return $exe }
  }
  $onPath = Get-Command "$($App.Id).exe" -ErrorAction SilentlyContinue
  if ($onPath) { return $onPath.Source }
  return $null
}

function Get-OllamaExe {
  $exe = Join-Path $env:LOCALAPPDATA "Programs\Ollama\ollama.exe"
  if (Test-Path $exe) { return $exe }
  $cmd = Get-Command ollama -ErrorAction SilentlyContinue
  if ($cmd) { return $cmd.Source }
  return $null
}

function Get-ClaudeExe {
  $cmd = Get-Command claude -ErrorAction SilentlyContinue
  if ($cmd) { return $cmd.Source }
  $exe = Join-Path $env:USERPROFILE ".local\bin\claude.exe"
  if (Test-Path $exe) { return $exe }
  return $null
}

# ---------------------------------------------------------------------------
# Plan
# ---------------------------------------------------------------------------

Say "Easel setup  ($(Get-Date -Format 'yyyy-MM-dd HH:mm'))   log: $LogFile" "White"

Step "Looking at this PC"
$os = Get-CimInstance Win32_OperatingSystem
$cs = Get-CimInstance Win32_ComputerSystem
$ramGB = [math]::Round($cs.TotalPhysicalMemory / 1GB)
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x64" }
$drive = (Split-Path -Qualifier $InstallDir).TrimEnd(":")
$freeGB = [math]::Round((Get-PSDrive -Name $drive).Free / 1GB)
$gpus = @(Get-Gpus)
$gpu = $gpus | Sort-Object VramGB -Descending | Select-Object -First 1

Say "   Windows:  $($os.Caption) (build $($os.BuildNumber), $arch)"
Say "   Memory:   $ramGB GB"
Say "   Disk:     $freeGB GB free on ${drive}: (install folder $InstallDir)"
if ($gpus.Count -eq 0) { Say "   Graphics: none found" }
foreach ($g in $gpus) { Say ("   Graphics: {0} ({1} GB)" -f $g.Name, $g.VramGB) }

$vram = if ($gpu) { [double]$gpu.VramGB } else { 0 }
$comfyBuild = "cpu"
$comfyNote = "no supported graphics card, so ComfyUI runs on the CPU (slow; Gemini is the better choice here)"
if ($gpu -and $gpu.Vendor -eq "nvidia" -and $vram -ge 4) {
  $comfyBuild = "nvidia"; $comfyNote = "NVIDIA build"
} elseif ($gpu -and $gpu.Vendor -eq "amd" -and (Test-AmdSupported $gpu.Name)) {
  $comfyBuild = "amd"; $comfyNote = "AMD build (ROCm)"
} elseif ($gpu -and $gpu.Vendor -eq "amd") {
  $comfyNote = "this Radeon is older than the RX 7000 series, which ComfyUI's AMD build needs, so it runs on the CPU"
} elseif ($gpu -and $gpu.Vendor -eq "intel") {
  $comfyNote = "ComfyUI has no ready-made Intel build, so it runs on the CPU"
}

$gpuForModels = if ($comfyBuild -eq "cpu") { 0 } else { $vram }
$imageModel = $ImageModels | Where-Object { $gpuForModels -ge $_.MinVramGB } | Select-Object -First 1
$brainBasis = if ($comfyBuild -ne "cpu") { $vram } else { [math]::Floor($ramGB / 3) }
$brainModel = $BrainModels | Where-Object { $brainBasis -ge $_.MinGB } | Select-Object -First 1

$easelExe = Get-EaselInstall
$easelInstaller = Get-ChildItem -Path $PSScriptRoot -Filter "Easel_*-setup.exe" -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -Last 1
$comfyDir = Join-Path $InstallDir "ComfyUI"
$comfyInstalled = [bool](Get-ChildItem -Path $comfyDir -Filter "run_*.bat" -Recurse -Depth 2 -ErrorAction SilentlyContinue | Select-Object -First 1)
$ollamaExe = Get-OllamaExe
$claudeExe = Get-ClaudeExe
$gitInstalled = Test-Command "git"

$plan = New-Object System.Collections.ArrayList
function Add-Plan([string]$Key, [string]$What, [double]$SizeGB, [bool]$Done) {
  [void]$plan.Add([pscustomobject]@{ Key = $Key; What = $What; SizeGB = $SizeGB; Done = $Done })
}
Add-Plan "easel" "Easel" 0.01 ([bool]$easelExe)
if (-not $SkipApps) {
  foreach ($app in $CraftApps) { Add-Plan "craft:$($app.Id)" "$($app.Name) (instead of $($app.Replaces))" 0.1 ([bool](Get-CraftInstall $app)) }
}
if (-not $SkipComfy) {
  Add-Plan "comfy" "ComfyUI, $comfyNote" 3.0 $comfyInstalled
  if (-not $SkipModels -and $imageModel) {
    $modelPath = Join-Path $comfyDir "ComfyUI\models\checkpoints\$($imageModel.File)"
    $modelDone = [bool](Get-ChildItem -Path $comfyDir -Filter $imageModel.File -Recurse -Depth 5 -ErrorAction SilentlyContinue | Select-Object -First 1)
    Add-Plan "model" "Image model: $($imageModel.Name)" $imageModel.SizeGB $modelDone
  }
}
if (-not $SkipLocalBrain) {
  Add-Plan "ollama" "Ollama (runs the local brain)" 1.0 ([bool]$ollamaExe)
  if (-not $SkipModels) { Add-Plan "brainmodel" "Local brain model: $($brainModel.Model)" $brainModel.SizeGB $false }
}
if (-not $SkipClaude) {
  Add-Plan "git" "Git for Windows (Claude Code needs it)" 0.3 $gitInstalled
  Add-Plan "claude" "Claude Code (the brain that drives the editors)" 0.2 ([bool]$claudeExe)
}
Add-Plan "settings" "Point Easel at all of the above" 0 $false

Step "The plan"
$needGB = 0
foreach ($item in $plan) {
  if ($item.Done) { Skip "$($item.What)  (already installed)" }
  else {
    Say ("   ->  {0}{1}" -f $item.What, $(if ($item.SizeGB -ge 0.5) { "  (~{0} GB)" -f $item.SizeGB } else { "" })) "White"
    $needGB += $item.SizeGB
  }
}
Say ("   About {0} GB to download and store; {1} GB free." -f [math]::Ceiling($needGB), $freeGB)
if ($needGB + 5 -gt $freeGB) { Warn "That's tight on space. Free up room, or use -InstallDir on a bigger drive, or -SkipModels." }
if (-not $easelExe -and -not $easelInstaller) { Warn "No Easel installer next to this script; Easel itself will be skipped. Keep this script in the same folder as Easel_..._x64-setup.exe." }

if ($DryRun) {
  if ($CheckLinks) {
    Step "Checking every download link"
    $bad = 0
    $check = { param($label, $ok) if ($ok) { Ok $label } else { Warn "NOT reachable: $label"; $script:bad++ } }
    foreach ($app in $CraftApps) {
      try {
        $rel = Get-LatestRelease "storytold/$($app.Id)"
        $asset = Find-Asset $rel @("^$($app.Id)-.*-windows-$arch\.msi$", "windows-$arch.*\.msi$", "\.msi$")
        & $check "$($app.Name) $($rel.tag_name): $(if ($asset) { $asset.name } else { 'no Windows installer in the latest release' })" ([bool]$asset)
      } catch { & $check "$($app.Name): $($_.Exception.Message)" $false }
    }
    try {
      $rel = Get-LatestRelease $ComfyRepo
      foreach ($build in @("nvidia", "amd")) {
        $asset = Find-Asset $rel @("windows_portable_$build.*\.7z$")
        & $check "ComfyUI $($rel.tag_name) $build build: $(if ($asset) { $asset.name } else { 'missing' })" ([bool]$asset)
      }
    } catch { & $check "ComfyUI releases: $($_.Exception.Message)" $false }
    foreach ($m in $ImageModels) { & $check "$($m.Name): $($m.Url)" (Test-Url $m.Url) }
    foreach ($u in @($SevenZipUrl, $OllamaSetupUrl, $ClaudeInstallUrl)) { & $check $u (Test-Url $u) }
    if ($bad -gt 0) { Say "$bad link(s) failed." "Red"; exit 1 }
    Ok "All links work."
  }
  Say ""; Say "Dry run: nothing was changed." "White"
  exit 0
}

if (-not (Test-Admin)) {
  Say ""; Say "Some installers need administrator rights. Windows will ask for permission next." "White"
  $argsList = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "`"$PSCommandPath`"")
  foreach ($kv in $PSBoundParameters.GetEnumerator()) {
    if ($kv.Value -is [switch]) { if ($kv.Value) { $argsList += "-$($kv.Key)" } } else { $argsList += "-$($kv.Key)"; $argsList += "`"$($kv.Value)`"" }
  }
  Start-Process powershell.exe -Verb RunAs -ArgumentList $argsList
  exit 0
}

if (-not $Yes) {
  Say ""
  $answer = Read-Host "Go ahead? [Y/n]"
  if ($answer -and $answer -notmatch "^(y|yes)$") { Say "Nothing was changed."; exit 0 }
}

$failures = New-Object System.Collections.ArrayList
function Invoke-Step([string]$Key, [scriptblock]$Action) {
  $item = $plan | Where-Object { $_.Key -eq $Key } | Select-Object -First 1
  if (-not $item) { return }
  if ($item.Done) { Skip "$($item.What): already installed"; return }
  Step $item.What
  try { & $Action; $item.Done = $true; Ok $item.What }
  catch { Warn "$($item.What) failed: $($_.Exception.Message)"; [void]$failures.Add($item.What) }
}

$downloads = Join-Path $InstallDir "downloads"
New-Item -ItemType Directory -Force -Path $downloads | Out-Null

# ---------------------------------------------------------------------------
# Install
# ---------------------------------------------------------------------------

Invoke-Step "easel" {
  if (-not $easelInstaller) { throw "no Easel installer next to this script" }
  $p = Start-Process $easelInstaller.FullName -ArgumentList "/S" -Wait -PassThru
  if ($p.ExitCode -ne 0) { throw "the Easel installer exited with code $($p.ExitCode)" }
  $script:easelExe = Get-EaselInstall
}

foreach ($app in $CraftApps) {
  Invoke-Step "craft:$($app.Id)" {
    $rel = Get-LatestRelease "storytold/$($app.Id)"
    $asset = Find-Asset $rel @("^$($app.Id)-.*-windows-$arch\.msi$", "windows-$arch.*\.msi$", "\.msi$")
    if (-not $asset) { throw "the latest $($app.Name) release has no Windows installer" }
    $file = Join-Path $downloads $asset.name
    Get-File $asset.browser_download_url $file
    Install-Msi $file
  }
}

Invoke-Step "comfy" {
  $rel = Get-LatestRelease $ComfyRepo
  # The NVIDIA package also carries the CPU launcher, so it's the CPU fallback too.
  $wanted = if ($comfyBuild -eq "amd") { "amd" } else { "nvidia" }
  $asset = Find-Asset $rel @("windows_portable_$wanted.*\.7z$")
  if (-not $asset) { throw "the latest ComfyUI release has no $wanted Windows package" }
  $archive = Join-Path $downloads $asset.name
  Get-File $asset.browser_download_url $archive
  $sevenZip = Join-Path $downloads "7zr.exe"
  if (-not (Test-Path $sevenZip)) { Get-File $SevenZipUrl $sevenZip }
  New-Item -ItemType Directory -Force -Path $comfyDir | Out-Null
  & $sevenZip x $archive "-o$comfyDir" -y | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "couldn't unpack $($asset.name) (7-Zip code $LASTEXITCODE)" }
}

# Find the unpacked ComfyUI folder and the launcher that suits this PC.
$comfyRoot = $null
$comfyLauncher = $null
$bats = @(Get-ChildItem -Path $comfyDir -Filter "run_*.bat" -Recurse -Depth 2 -ErrorAction SilentlyContinue)
if ($bats.Count -gt 0) {
  $prefer = switch ($comfyBuild) { "nvidia" { @("nvidia", "gpu") } "amd" { @("amd", "gpu") } default { @("cpu") } }
  foreach ($word in $prefer) {
    $comfyLauncher = $bats | Where-Object { $_.Name -match $word -and $_.Name -notmatch "fast|fp16" } | Select-Object -First 1
    if ($comfyLauncher) { break }
  }
  if (-not $comfyLauncher) { $comfyLauncher = $bats | Select-Object -First 1 }
  $comfyRoot = $comfyLauncher.DirectoryName
}

Invoke-Step "model" {
  if (-not $comfyRoot) { throw "ComfyUI isn't installed, so there's nowhere to put the model" }
  $dest = Join-Path $comfyRoot "ComfyUI\models\checkpoints\$($imageModel.File)"
  Get-File $imageModel.Url $dest
}

if ($comfyLauncher) {
  $desktop = [Environment]::GetFolderPath("Desktop")
  New-Shortcut (Join-Path $desktop "Start ComfyUI.lnk") $comfyLauncher.FullName "" $comfyRoot "Starts ComfyUI for Easel"
  if (-not $NoAutostart) {
    # Starts minimized at sign-in so Easel's ComfyUI engine is always ready.
    $startup = [Environment]::GetFolderPath("Startup")
    New-Shortcut (Join-Path $startup "ComfyUI for Easel.lnk") "cmd.exe" "/c start /min `"ComfyUI`" `"$($comfyLauncher.FullName)`"" $comfyRoot "Starts ComfyUI for Easel"
  }
}

Invoke-Step "ollama" {
  if (Test-Command "winget") { Install-Winget "Ollama.Ollama" }
  else {
    $setup = Join-Path $downloads "OllamaSetup.exe"
    Get-File $OllamaSetupUrl $setup
    $p = Start-Process $setup -ArgumentList "/VERYSILENT", "/NORESTART" -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "the Ollama installer exited with code $($p.ExitCode)" }
  }
  $script:ollamaExe = Get-OllamaExe
  if (-not $script:ollamaExe) { throw "Ollama installed but ollama.exe wasn't found" }
}

Invoke-Step "brainmodel" {
  if (-not $ollamaExe) { throw "Ollama isn't installed" }
  try { Invoke-RestMethod "http://127.0.0.1:11434/api/version" -TimeoutSec 3 | Out-Null }
  catch { Start-Process $ollamaExe -ArgumentList "serve" -WindowStyle Hidden; Start-Sleep -Seconds 5 }
  & $ollamaExe pull $brainModel.Model
  if ($LASTEXITCODE -ne 0) { throw "ollama pull $($brainModel.Model) failed" }
}

Invoke-Step "git" { Install-Winget "Git.Git" }

Invoke-Step "claude" {
  # Claude Code's official Windows installer, run in a fresh PowerShell.
  $p = Start-Process powershell.exe -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "irm $ClaudeInstallUrl | iex") -Wait -PassThru -NoNewWindow
  if ($p.ExitCode -ne 0) { throw "the Claude Code installer exited with code $($p.ExitCode)" }
  # The installer leaves ~/.local/bin off PATH, so `claude` wouldn't work in a terminal.
  $bin = Join-Path $env:USERPROFILE ".local\bin"
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  if (($userPath -split ";") -notcontains $bin) {
    [Environment]::SetEnvironmentVariable("Path", (($userPath.TrimEnd(";"), $bin) -join ";").TrimStart(";"), "User")
  }
  Update-Path
  $script:claudeExe = Get-ClaudeExe
  if (-not $script:claudeExe) { throw "Claude Code installed but claude.exe wasn't found" }
}

# ---------------------------------------------------------------------------
# Point Easel at everything. Existing settings and keys are kept; only empty
# or missing values are filled in.
# ---------------------------------------------------------------------------

Invoke-Step "settings" {
  $settingsPath = Join-Path $env:APPDATA "app.easel.studio\settings.json"
  $settings = $null
  if (Test-Path $settingsPath) {
    try { $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json } catch { Warn "Existing settings couldn't be read; a backup is kept as settings.json.bak"; Copy-Item $settingsPath "$settingsPath.bak" -Force }
  }
  if (-not $settings) { $settings = [pscustomobject]@{} }
  foreach ($section in @("comfy", "brain", "apps")) { if (-not (Get-Prop $settings $section)) { Set-Prop $settings $section ([pscustomobject]@{}) } }

  $comfy = $settings.comfy
  if (-not (Get-Prop $comfy "url")) { Set-Prop $comfy "url" "http://127.0.0.1:8188" }
  if ($imageModel -and $comfyRoot -and -not (Get-Prop $comfy "checkpoint")) {
    Set-Prop $comfy "checkpoint" $imageModel.File
    Set-Prop $comfy "steps" $imageModel.Steps
    Set-Prop $comfy "cfg" $imageModel.Cfg
    Set-Prop $comfy "sampler" $imageModel.Sampler
    Set-Prop $comfy "scheduler" $imageModel.Scheduler
    Set-Prop $comfy "baseSize" $imageModel.BaseSize
    Set-Prop $comfy "negative" $imageModel.Negative
  }

  $brain = $settings.brain
  if ($ollamaExe -and -not (Get-Prop $brain "localModel")) {
    Set-Prop $brain "localUrl" "http://127.0.0.1:11434"
    Set-Prop $brain "localModel" $brainModel.Model
  }
  if ($claudeExe -and -not (Get-Prop $brain "claudeCliPath")) { Set-Prop $brain "claudeCliPath" $claudeExe }
  if (-not (Get-Prop $brain "order")) {
    # The free local brain plans first; Claude Code is the fallback and the advisor.
    $order = @()
    if ($ollamaExe) { $order += "local" }
    if ($claudeExe) { $order += "claudeCli" }
    Set-Prop $brain "order" $order
    if ($claudeExe) {
      Set-Prop $brain "advisorOrder" @("claudeCli")
      Set-Prop $brain "advisorMode" "auto"
    }
  }

  foreach ($app in $CraftApps) {
    $exe = Get-CraftInstall $app
    if ($exe -and -not (Get-Prop $settings.apps $app.Id)) {
      Set-Prop $settings.apps $app.Id ([pscustomobject]@{ gui = $exe; cli = (Join-Path (Split-Path $exe) "$($app.Id)-cli.exe") })
    }
  }
  Write-JsonNoBom $settingsPath $settings
}

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------

Step "Finished"
if ($failures.Count -gt 0) {
  Warn "These steps didn't finish: $($failures -join ', '). Run this script again to retry them; the log is $LogFile"
  $global:LASTEXITCODE = 2
} else {
  Ok "Everything is set up."
}
if ($comfyLauncher -and -not $NoLaunch) {
  Start-Process cmd.exe -ArgumentList "/c start /min `"ComfyUI`" `"$($comfyLauncher.FullName)`"" -WorkingDirectory $comfyRoot
  Say "   ComfyUI is starting (first start takes a minute). It also starts at sign-in$(if ($NoAutostart) { ' -- turned off by -NoAutostart' })."
}
Say ""
Say "Last steps, once:" "White"
if ($claudeExe) { Say "   1. Sign in to Claude Code: a window opens now; follow its sign-in, then close it." }
Say "   2. In Easel, open Settings > Gemini and paste your Gemini API key (aistudio.google.com) for Nano Banana and the Gemini 4 brain."
Say "   3. Antigravity (agy) isn't installed by this script; get it from Google if you want it as a brain."
if (-not $NoLaunch) {
  if ($claudeExe) { Start-Process powershell.exe -ArgumentList @("-NoExit", "-Command", "& '$claudeExe'") }
  if ($easelExe) { Start-Process $easelExe }
}
if (-not $Yes) { Read-Host "Press Enter to close" | Out-Null }
if ($failures.Count -gt 0) { exit 2 }
