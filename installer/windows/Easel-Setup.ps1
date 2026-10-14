<#
.SYNOPSIS
  Sets up Easel and everything around it on a Windows PC, sized to its hardware.

.DESCRIPTION
  1. Detects this PC (graphics card, its real memory and NVIDIA compute
     capability, RAM, disk, running model servers, installed tools).
  2. Resolves that into a profile (EaselProfile.ps1, the same rules as the
     Linux resolver, tested against installer/profile-cases.json).
  3. Shows the plan and, after you say yes, installs from the profile:
       - Easel (from the installer next to this script)
       - the seven Crafting Apps
       - ComfyUI for NVIDIA cards: the latest build for RTX and GTX 16xx; for
         Pascal and older, ComfyUI pinned to v0.7.0 with PyTorch 2.6.0+cu124
       - the checkpoint that fits the graphics memory (DreamShaper, SDXL or Flux)
       - the local brain: Ollama (or an LM Studio / Ollama already running), or
         on AMD + Windows llama.cpp's Vulkan build with a GGUF model
       - Git and Claude Code
       - Easel's settings, written from the profile, and the hardware profile
         Easel reads to warn before ComfyUI jobs this GPU can't run

  Running it again is safe: anything already installed is skipped, and
  existing Easel settings and keys are kept.

.PARAMETER DryRun
  Only detect and show the plan. Changes nothing.

.PARAMETER CheckLinks
  With -DryRun: also check that every download the plan needs is reachable.

.PARAMETER FactsFile
  Use hardware facts from this JSON file instead of detecting (for testing a
  plan for another PC, e.g. the "facts" of a case in profile-cases.json).

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
  [string]$FactsFile = "",
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
. (Join-Path $PSScriptRoot "EaselProfile.ps1")

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
$LlamaCppRepo = "ggml-org/llama.cpp"
$SevenZipUrl = "https://www.7-zip.org/a/7zr.exe"
$OllamaSetupUrl = "https://ollama.com/download/OllamaSetup.exe"
$ClaudeInstallUrl = "https://claude.ai/install.ps1"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA "EaselSuite" }
$SuiteDir = Join-Path $env:LOCALAPPDATA "EaselSuite"
New-Item -ItemType Directory -Force -Path $SuiteDir | Out-Null
$LogFile = Join-Path $SuiteDir "setup.log"

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

# GitHub's API allows only 60 anonymous requests an hour per IP address, and
# shared connections run out. So: use a token when one is set (CI has one),
# and when the API refuses, read the release's public download page instead.
$script:ReleaseCache = @{}
function Get-Release([string]$Repo, [string]$Tag = "") {
  $key = "$Repo@$Tag"
  if ($script:ReleaseCache.ContainsKey($key)) { return $script:ReleaseCache[$key] }
  $release = $null
  if (-not $env:EASEL_SKIP_GITHUB_API) {
    $headers = @{ "User-Agent" = "Easel-Setup"; "Accept" = "application/vnd.github+json" }
    if ($env:GITHUB_TOKEN) { $headers["Authorization"] = "Bearer $env:GITHUB_TOKEN" }
    $path = if ($Tag) { "releases/tags/$Tag" } else { "releases/latest" }
    try { $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/$path" -Headers $headers -TimeoutSec 60 }
    catch { Say "   (GitHub's API said no for $Repo; reading its release page instead)" "DarkGray" }
  }
  if (-not $release) { $release = Get-ReleaseFromPage $Repo $Tag }
  $script:ReleaseCache[$key] = $release
  return $release
}

# The same information from github.com's pages, which aren't rate limited like the API.
function Get-ReleaseFromPage([string]$Repo, [string]$Tag) {
  if (-not $Tag) {
    # /releases/latest redirects to /releases/tag/<tag>.
    $request = [Net.HttpWebRequest]::Create("https://github.com/$Repo/releases/latest")
    $request.AllowAutoRedirect = $false
    $request.UserAgent = "Easel-Setup"
    $response = $request.GetResponse()
    $location = $response.Headers["Location"]
    $response.Close()
    if (-not $location -or $location -notmatch "/releases/tag/([^/?#]+)") { throw "couldn't find the latest release of $Repo" }
    $Tag = [Uri]::UnescapeDataString($Matches[1])
  }
  $html = (Invoke-WebRequest -Uri "https://github.com/$Repo/releases/expanded_assets/$Tag" -UseBasicParsing -TimeoutSec 60 -Headers @{ "User-Agent" = "Easel-Setup" }).Content
  $assets = @()
  $pattern = 'href="(/' + [Regex]::Escape($Repo) + '/releases/download/[^"]+)"'
  foreach ($m in [Regex]::Matches($html, $pattern, "IgnoreCase")) {
    $href = $m.Groups[1].Value
    $name = [Uri]::UnescapeDataString(($href -split "/")[-1])
    if (-not ($assets | Where-Object { $_.name -eq $name })) {
      $assets += [pscustomobject]@{ name = $name; browser_download_url = "https://github.com$href" }
    }
  }
  if ($assets.Count -eq 0) { throw "the $Repo $Tag release page lists no downloads" }
  [pscustomobject]@{ tag_name = $Tag; assets = $assets }
}

# The newest release (looking back through recent ones) that has a matching
# file. llama.cpp's "latest" release doesn't always carry every build, so
# trusting it alone breaks installs.
function Find-ReleaseAsset([string]$Repo, [string[]]$Patterns) {
  $candidates = @()
  if (-not $env:EASEL_SKIP_GITHUB_API) {
    $headers = @{ "User-Agent" = "Easel-Setup"; "Accept" = "application/vnd.github+json" }
    if ($env:GITHUB_TOKEN) { $headers["Authorization"] = "Bearer $env:GITHUB_TOKEN" }
    try { $candidates = @(@(Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases?per_page=20" -Headers $headers -TimeoutSec 60) | Where-Object { -not $_.draft }) }
    catch { Say "   (GitHub's API said no for $Repo; reading its release pages instead)" "DarkGray" }
  }
  foreach ($release in $candidates) {
    $asset = Find-Asset $release $Patterns
    if ($asset) { return [pscustomobject]@{ Release = $release; Asset = $asset } }
  }
  if ($candidates.Count -eq 0) {
    # No API: the public releases page lists the newest tags; open each one's downloads.
    $tags = @()
    try {
      $html = (Invoke-WebRequest -Uri "https://github.com/$Repo/releases" -UseBasicParsing -TimeoutSec 60 -Headers @{ "User-Agent" = "Easel-Setup" }).Content
      foreach ($m in [Regex]::Matches($html, '/' + [Regex]::Escape($Repo) + '/releases/tag/([^"/?#]+)', "IgnoreCase")) {
        $t = [Uri]::UnescapeDataString($m.Groups[1].Value)
        if ($tags -notcontains $t) { $tags += $t }
      }
    } catch { }
    if ($tags.Count -eq 0) { $candidates = @(Get-Release $Repo) }
    foreach ($t in ($tags | Select-Object -First 10)) {
      try { $release = Get-ReleaseFromPage $Repo $t } catch { continue }
      $candidates += $release
      $asset = Find-Asset $release $Patterns
      if ($asset) { return [pscustomobject]@{ Release = $release; Asset = $asset } }
    }
    foreach ($release in $candidates) {
      $asset = Find-Asset $release $Patterns
      if ($asset) { return [pscustomobject]@{ Release = $release; Asset = $asset } }
    }
  }
  $names = @($candidates | Select-Object -First 1 | ForEach-Object { $_.assets } | ForEach-Object { $_.name }) -join ", "
  throw "no release of $Repo has a matching download. The newest one has: $names"
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
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Destination) | Out-Null
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

function Add-UserPath([string]$Dir) {
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  if (-not $userPath) { $userPath = "" }
  if (($userPath -split ";") -notcontains $Dir) {
    [Environment]::SetEnvironmentVariable("Path", (($userPath.TrimEnd(";"), $Dir) -join ";").TrimStart(";"), "User")
  }
  Update-Path
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
  [IO.File]::WriteAllText($Path, ($Object | ConvertTo-Json -Depth 20), (New-Object Text.UTF8Encoding($false)))
}

function Set-Prop($Object, [string]$Name, $Value) {
  if ($Object.PSObject.Properties[$Name]) { $Object.$Name = $Value } else { $Object | Add-Member -NotePropertyName $Name -NotePropertyValue $Value }
}

function Get-Prop($Object, [string]$Name) {
  if ($Object -and $Object.PSObject.Properties[$Name]) { return $Object.$Name }
  return $null
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
  # Prefer the native claude.exe; an npm claude.cmd works too (Easel resolves it).
  $exe = Join-Path $env:USERPROFILE ".local\bin\claude.exe"
  if (Test-Path $exe) { return $exe }
  return (Find-EaselTool "claude")
}

# The ComfyUI folder that holds run_*.bat, and the launcher that suits this PC.
function Find-ComfyLauncher([string]$Dir, [string]$Backend) {
  $bats = @(Get-ChildItem -Path $Dir -Filter "run_*.bat" -Recurse -Depth 2 -ErrorAction SilentlyContinue)
  if ($bats.Count -eq 0) { return $null }
  $prefer = if ($Backend -eq "cuda" -or $Backend -eq "cuda-legacy") { @("nvidia", "gpu") } else { @("cpu") }
  foreach ($word in $prefer) {
    $hit = $bats | Where-Object { $_.Name -match $word -and $_.Name -notmatch "fast|fp16" } | Select-Object -First 1
    if ($hit) { return $hit }
  }
  return ($bats | Select-Object -First 1)
}

# ---------------------------------------------------------------------------
# Detect and resolve
# ---------------------------------------------------------------------------

Say "Easel setup  ($(Get-Date -Format 'yyyy-MM-dd HH:mm'))   log: $LogFile" "White"
Step "Looking at this PC"
$catalog = Get-EaselCatalog
if ($FactsFile) {
  $facts = Get-Content $FactsFile -Raw | ConvertFrom-Json
  if ($facts.PSObject.Properties["facts"]) { $facts = $facts.facts }
  Warn "Using hardware facts from $FactsFile instead of this PC's."
} else {
  $facts = Get-EaselHardwareFacts $InstallDir
}
$hw = Resolve-EaselProfile $facts $catalog
$arch = if ($facts.arch) { $facts.arch } else { "x64" }
# llama.cpp has named this build "llama-bNNNN-bin-win-vulkan-x64.zip"; match loosely in case that shifts.
$archWord = if ($arch -eq "arm64") { "arm64" } else { "(x64|amd64|x86_64)" }
$LlamaVulkanPatterns = @("bin-win-vulkan-$archWord\.zip$", "win.*vulkan.*$archWord.*\.zip$", "vulkan.*win.*$archWord.*\.zip$")

Say "   Memory:   $($facts.ramGB) GB, $($facts.cpuName)"
Say "   Disk:     $($facts.freeGB) GB free (install folder $InstallDir)"
if (@($facts.gpus).Count -eq 0) { Say "   Graphics: none found" }
foreach ($g in @($facts.gpus)) {
  $capText = if ($null -ne $g.computeCap) { ", compute {0}" -f $g.computeCap } else { "" }
  Say ("   Graphics: {0} ({1} GB{2})" -f $g.name, $g.vramGB, $capText)
}
$backendText = @{
  "cuda" = "NVIDIA, current PyTorch"
  "cuda-legacy" = "NVIDIA Pascal or older: ComfyUI $($catalog.legacyNvidia.comfyTag) with PyTorch $($catalog.legacyNvidia.torch)"
  "rocm" = "AMD with ROCm"
  "cloud" = "AMD on Windows: Gemini for images, llama.cpp (Vulkan) for the local brain"
  "cpu" = "no usable GPU: Gemini for images, the local brain on the CPU"
}[$hw.gpuBackend]
Say "   Backend:  $backendText" "White"
foreach ($w in @($hw.warnings)) { Warn $w }

$comfyDir = Join-Path $InstallDir "ComfyUI"
$llamaDir = Join-Path $InstallDir "llama.cpp"
$llmDir = Join-Path $InstallDir "models"
$easelExe = Get-EaselInstall
$easelInstaller = Get-ChildItem -Path $PSScriptRoot -Filter "Easel_*-setup.exe" -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -Last 1
$ollamaExe = Get-OllamaExe
$claudeExe = Get-ClaudeExe
$llm = $hw.llm
$ckpt = $hw.checkpoint
$useComfy = $hw.comfy -and -not $SkipComfy

$plan = New-Object System.Collections.ArrayList
function Add-Plan([string]$Key, [string]$What, [double]$SizeGB, [bool]$Done) {
  [void]$plan.Add([pscustomobject]@{ Key = $Key; What = $What; SizeGB = $SizeGB; Done = $Done })
}
Add-Plan "easel" "Easel" 0.01 ([bool]$easelExe)
if (-not $SkipApps) {
  foreach ($app in $CraftApps) { Add-Plan "craft:$($app.Id)" "$($app.Name) (instead of $($app.Replaces))" 0.1 ([bool](Get-CraftInstall $app)) }
}
if ($useComfy) {
  $comfyLabel = if ($hw.comfy.pinTag) { "ComfyUI $($hw.comfy.pinTag) (pinned for this GPU)" } else { "ComfyUI (latest, NVIDIA build)" }
  Add-Plan "comfy" $comfyLabel 2.5 ([bool](Find-ComfyLauncher $comfyDir $hw.gpuBackend))
  if ($hw.comfy.pipPackages.Count -gt 0) { Add-Plan "torchpin" "PyTorch $($hw.comfy.torch) inside ComfyUI" 2.5 $false }
  if (-not $SkipModels -and $ckpt) {
    $have = [bool](Get-ChildItem -Path $comfyDir -Filter $ckpt.file -Recurse -Depth 5 -ErrorAction SilentlyContinue | Select-Object -First 1)
    Add-Plan "checkpoint" "Image model: $($ckpt.name)" $ckpt.sizeGB $have
  }
} elseif (-not $hw.comfy) {
  Skip "ComfyUI: not installed on this PC; images use Gemini (add your key in Easel)"
}
if (-not $SkipLocalBrain) {
  switch ($llm.runtime) {
    "ollama" {
      Add-Plan "ollama" "Ollama (runs the local brain)" 1.0 ([bool]$ollamaExe)
      if (-not $SkipModels) { Add-Plan "brainmodel" "Local brain: $($llm.model), $($llm.contextLength) token context" $llm.sizeGB $false }
    }
    "lmstudio" { Skip "Local brain: LM Studio is already running; Easel will use it" }
    "llamacpp-vulkan" {
      Add-Plan "llamacpp" "llama.cpp (Vulkan build) for the local brain" 0.1 (Test-Path (Join-Path $llamaDir "llama-server.exe"))
      if (-not $SkipModels) {
        $gguf = Join-Path $llmDir (Split-Path -Leaf $llm.ggufUrl)
        Add-Plan "gguf" "Local brain: $($llm.model) (GGUF), $($llm.contextLength) token context" $llm.sizeGB (Test-Path $gguf)
      }
    }
  }
}
if (-not $SkipClaude) {
  Add-Plan "git" "Git for Windows (Claude Code needs it)" 0.3 (Test-Command "git")
  Add-Plan "claude" "Claude Code (the brain that drives the editors)" 0.2 ([bool]$claudeExe)
}
Add-Plan "settings" "Write Easel's settings and hardware profile" 0 $false

Step "The plan"
$needGB = 0
foreach ($item in $plan) {
  if ($item.Done) { Skip "$($item.What)  (already installed)" }
  else {
    Say ("   ->  {0}{1}" -f $item.What, $(if ($item.SizeGB -ge 0.5) { "  (~{0} GB)" -f $item.SizeGB } else { "" })) "White"
    $needGB += $item.SizeGB
  }
}
Say ("   About {0} GB to download and store; {1} GB free." -f [math]::Ceiling($needGB), $facts.freeGB)
if ($needGB + 5 -gt [double]$facts.freeGB) { Warn "That's tight on space. Free up room, use -InstallDir on a bigger drive, or -SkipModels." }
if (-not $easelExe -and -not $easelInstaller) { Warn "No Easel installer next to this script; Easel itself will be skipped." }

if ($DryRun) {
  if ($CheckLinks) {
    Step "Checking every download link"
    $script:bad = 0
    $check = { param($label, $ok) if ($ok) { Ok $label } else { Warn "NOT reachable: $label"; $script:bad++ } }
    foreach ($app in $CraftApps) {
      try {
        $rel = Get-Release "storytold/$($app.Id)"
        $asset = Find-Asset $rel @("^$($app.Id)-.*-windows-$arch\.msi$", "windows-$arch.*\.msi$", "\.msi$")
        & $check "$($app.Name) $($rel.tag_name): $(if ($asset) { $asset.name } else { 'no Windows installer' })" ([bool]$asset)
      } catch { & $check "$($app.Name): $($_.Exception.Message)" $false }
    }
    foreach ($tag in @("", $catalog.legacyNvidia.comfyTag)) {
      try {
        $rel = Get-Release $ComfyRepo $tag
        $asset = Find-Asset $rel @("windows_portable_nvidia.*\.7z$")
        & $check "ComfyUI $($rel.tag_name) NVIDIA build: $(if ($asset) { $asset.name } else { 'missing' })" ([bool]$asset)
      } catch { & $check "ComfyUI release '$tag': $($_.Exception.Message)" $false }
    }
    try {
      $found = Find-ReleaseAsset $LlamaCppRepo $LlamaVulkanPatterns
      & $check "llama.cpp $($found.Release.tag_name) Vulkan build: $($found.Asset.name)" $true
    } catch { & $check "llama.cpp Vulkan build: $($_.Exception.Message)" $false }
    foreach ($c in $catalog.checkpoints.PSObject.Properties) { & $check "$($c.Value.name): $($c.Value.url)" (Test-Url $c.Value.url) }
    foreach ($l in $catalog.llms.PSObject.Properties) { & $check "$($l.Name) GGUF: $($l.Value.ggufUrl)" (Test-Url $l.Value.ggufUrl) }
    & $check "PyTorch $($catalog.legacyNvidia.torch) index" (Test-Url ($catalog.legacyNvidia.torchIndex.TrimEnd("/") + "/torch/"))
    foreach ($u in @($SevenZipUrl, $OllamaSetupUrl, $ClaudeInstallUrl)) { & $check $u (Test-Url $u) }
    if ($script:bad -gt 0) { Say "$($script:bad) link(s) failed." "Red"; exit 1 }
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
    $rel = Get-Release "storytold/$($app.Id)"
    $asset = Find-Asset $rel @("^$($app.Id)-.*-windows-$arch\.msi$", "windows-$arch.*\.msi$", "\.msi$")
    if (-not $asset) { throw "the latest $($app.Name) release has no Windows installer" }
    $file = Join-Path $downloads $asset.name
    Get-File $asset.browser_download_url $file
    Install-Msi $file
  }
}

Invoke-Step "comfy" {
  # Pascal and older get the pinned release; later ones need comfy_kitchen and PyTorch 2.12+.
  $rel = Get-Release $ComfyRepo ([string]$hw.comfy.pinTag)
  $asset = Find-Asset $rel @("windows_portable_nvidia.*\.7z$")
  if (-not $asset) { throw "ComfyUI $($rel.tag_name) has no NVIDIA Windows package" }
  $archive = Join-Path $downloads $asset.name
  Get-File $asset.browser_download_url $archive
  $sevenZip = Join-Path $downloads "7zr.exe"
  if (-not (Test-Path $sevenZip)) { Get-File $SevenZipUrl $sevenZip }
  New-Item -ItemType Directory -Force -Path $comfyDir | Out-Null
  & $sevenZip x $archive "-o$comfyDir" -y | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "couldn't unpack $($asset.name) (7-Zip code $LASTEXITCODE)" }
}

$comfyLauncher = if ($useComfy) { Find-ComfyLauncher $comfyDir $hw.gpuBackend } else { $null }
$comfyRoot = if ($comfyLauncher) { $comfyLauncher.DirectoryName } else { $null }

Invoke-Step "torchpin" {
  if (-not $comfyRoot) { throw "ComfyUI isn't installed" }
  $python = Join-Path $comfyRoot "python_embeded\python.exe"
  if (-not (Test-Path $python)) { throw "ComfyUI's python_embeded wasn't found" }
  $pipArgs = @("-m", "pip", "install", "--force-reinstall", "--no-cache-dir") + @($hw.comfy.pipPackages) + @("--index-url", $hw.comfy.torchIndex)
  & $python @pipArgs | Out-Host
  if ($LASTEXITCODE -ne 0) { throw "pip couldn't install PyTorch $($hw.comfy.torch)" }
  $version = (& $python -c "import torch; print(torch.__version__)").Trim()
  if ($version -ne $hw.comfy.torch) { throw "PyTorch reports $version, expected $($hw.comfy.torch)" }
  Say "   PyTorch $version is in place. Don't open the ComfyUI Desktop app on this PC: it reinstalls its own PyTorch."
}

Invoke-Step "checkpoint" {
  if (-not $comfyRoot) { throw "ComfyUI isn't installed, so there's nowhere to put the model" }
  Get-File $ckpt.url (Join-Path $comfyRoot "ComfyUI\models\checkpoints\$($ckpt.file)")
}

if ($comfyLauncher) {
  New-Shortcut (Join-Path ([Environment]::GetFolderPath("Desktop")) "Start ComfyUI.lnk") $comfyLauncher.FullName "" $comfyRoot "Starts ComfyUI for Easel"
  if (-not $NoAutostart) {
    New-Shortcut (Join-Path ([Environment]::GetFolderPath("Startup")) "ComfyUI for Easel.lnk") "cmd.exe" "/c start /min `"ComfyUI`" `"$($comfyLauncher.FullName)`"" $comfyRoot "Starts ComfyUI for Easel"
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
  # Ollama reads its context window from this; set it for the user so it sticks.
  [Environment]::SetEnvironmentVariable("OLLAMA_CONTEXT_LENGTH", [string]$llm.contextLength, "User")
  $env:OLLAMA_CONTEXT_LENGTH = [string]$llm.contextLength
  try { Invoke-RestMethod "http://127.0.0.1:11434/api/version" -TimeoutSec 3 | Out-Null }
  catch { Start-Process $ollamaExe -ArgumentList "serve" -WindowStyle Hidden; Start-Sleep -Seconds 5 }
  & $ollamaExe pull $llm.model
  if ($LASTEXITCODE -ne 0) { throw "ollama pull $($llm.model) failed" }
}

Invoke-Step "llamacpp" {
  $found = Find-ReleaseAsset $LlamaCppRepo $LlamaVulkanPatterns
  $asset = $found.Asset
  Say "   llama.cpp $($found.Release.tag_name): $($asset.name)"
  $zip = Join-Path $downloads $asset.name
  Get-File $asset.browser_download_url $zip
  New-Item -ItemType Directory -Force -Path $llamaDir | Out-Null
  Expand-Archive -Path $zip -DestinationPath $llamaDir -Force
  # Some releases put the files one folder down; bring llama-server.exe to the top.
  $server = Get-ChildItem -Path $llamaDir -Filter "llama-server.exe" -Recurse | Select-Object -First 1
  if (-not $server) { throw "llama-server.exe wasn't in $($asset.name)" }
  if ($server.DirectoryName -ne $llamaDir) { Copy-Item -Path (Join-Path $server.DirectoryName "*") -Destination $llamaDir -Recurse -Force }
}

Invoke-Step "gguf" {
  Get-File $llm.ggufUrl (Join-Path $llmDir (Split-Path -Leaf $llm.ggufUrl))
}

if ($llm.runtime -eq "llamacpp-vulkan" -and -not $SkipLocalBrain) {
  $serverExe = Join-Path $llamaDir "llama-server.exe"
  $gguf = Join-Path $llmDir (Split-Path -Leaf $llm.ggufUrl)
  if ((Test-Path $serverExe) -and (Test-Path $gguf)) {
    # Every layer on the GPU (-ngl 999); Vulkan works on Radeon cards without ROCm.
    $serverArgs = "-m `"$gguf`" -c $($llm.contextLength) -ngl 999 --host 127.0.0.1 --port 8080 --jinja"
    New-Shortcut (Join-Path ([Environment]::GetFolderPath("Desktop")) "Start local brain.lnk") $serverExe $serverArgs $llamaDir "Starts llama.cpp for Easel's local brain"
    if (-not $NoAutostart) {
      New-Shortcut (Join-Path ([Environment]::GetFolderPath("Startup")) "Easel local brain.lnk") "cmd.exe" "/c start /min `"local brain`" `"$serverExe`" $serverArgs" $llamaDir "Starts llama.cpp for Easel's local brain"
    }
  }
}

Invoke-Step "git" { Install-Winget "Git.Git" }

Invoke-Step "claude" {
  # Claude Code's official Windows installer, run in a fresh PowerShell.
  $p = Start-Process powershell.exe -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "irm $ClaudeInstallUrl | iex") -Wait -PassThru -NoNewWindow
  if ($p.ExitCode -ne 0) { throw "the Claude Code installer exited with code $($p.ExitCode)" }
  # The installer leaves ~/.local/bin off PATH, so `claude` wouldn't work in a terminal.
  Add-UserPath (Join-Path $env:USERPROFILE ".local\bin")
  $script:claudeExe = Get-ClaudeExe
  if (-not $script:claudeExe) { throw "Claude Code installed but claude.exe wasn't found" }
}

# ---------------------------------------------------------------------------
# Write Easel's settings from the profile. Existing settings and keys are
# kept; only empty or missing values are filled in.
# ---------------------------------------------------------------------------

Invoke-Step "settings" {
  $profileFile = [pscustomobject][ordered]@{ detected = $facts; resolved = $hw; summary = $hw.summary }
  Write-JsonNoBom (Join-Path $SuiteDir "hardware-profile.json") $profileFile

  $settingsPath = Join-Path $env:APPDATA "app.easel.studio\settings.json"
  $settings = $null
  if (Test-Path $settingsPath) {
    try { $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json }
    catch { Warn "Existing settings couldn't be read; a backup is kept as settings.json.bak"; Copy-Item $settingsPath "$settingsPath.bak" -Force }
  }
  if (-not $settings) { $settings = [pscustomobject]@{} }
  foreach ($section in @("comfy", "brain", "apps")) { if (-not (Get-Prop $settings $section)) { Set-Prop $settings $section ([pscustomobject]@{}) } }

  $comfy = $settings.comfy
  if (-not (Get-Prop $comfy "url")) { Set-Prop $comfy "url" "http://127.0.0.1:8188" }
  $ckptOnDisk = $ckpt -and $comfyRoot -and (Test-Path (Join-Path $comfyRoot "ComfyUI\models\checkpoints\$($ckpt.file)"))
  if ($ckptOnDisk -and -not (Get-Prop $comfy "checkpoint")) {
    Set-Prop $comfy "checkpoint" $ckpt.file
    Set-Prop $comfy "steps" $ckpt.steps
    Set-Prop $comfy "cfg" $ckpt.cfg
    Set-Prop $comfy "sampler" $ckpt.sampler
    Set-Prop $comfy "scheduler" $ckpt.scheduler
    Set-Prop $comfy "baseSize" $ckpt.baseSize
    Set-Prop $comfy "negative" $ckpt.negative
  }

  $brain = $settings.brain
  if (-not $SkipLocalBrain -and -not (Get-Prop $brain "localModel")) {
    Set-Prop $brain "localUrl" $llm.url
    # Ollama names the model; LM Studio and llama-server serve whatever is loaded.
    Set-Prop $brain "localModel" $(if ($llm.runtime -eq "ollama") { $llm.model } else { "" })
  }
  if ($claudeExe -and -not (Get-Prop $brain "claudeCliPath")) { Set-Prop $brain "claudeCliPath" $claudeExe }
  if (-not (Get-Prop $brain "order")) {
    $order = @($hw.brain.order | Where-Object { $_ -ne "claudeCli" -or $claudeExe })
    if ($SkipLocalBrain) { $order = @($order | Where-Object { $_ -ne "local" }) }
    Set-Prop $brain "order" $order
    if ($claudeExe) {
      Set-Prop $brain "advisorOrder" @($hw.brain.advisorOrder)
      Set-Prop $brain "advisorMode" $hw.brain.advisorMode
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
} else {
  Ok "Everything is set up."
}
if (-not $NoLaunch) {
  if ($comfyLauncher) {
    Start-Process cmd.exe -ArgumentList "/c start /min `"ComfyUI`" `"$($comfyLauncher.FullName)`"" -WorkingDirectory $comfyRoot
    Say "   ComfyUI is starting (first start takes a minute)."
  }
}
Say ""
Say "Last steps, once:" "White"
if ($claudeExe) { Say "   1. Sign in to Claude Code: a window opens now; follow its sign-in, then close it." }
Say "   2. In Easel, open Settings > Gemini and paste your Gemini API key (aistudio.google.com)$(if ($hw.imageEngine -eq 'gemini') { '. On this PC Gemini makes the images' })."
Say "   3. In Easel's Settings, press Test next to each brain to check it answers."
if (-not $NoLaunch) {
  if ($claudeExe) { Start-Process powershell.exe -ArgumentList @("-NoExit", "-Command", "& '$claudeExe'") }
  if ($easelExe) { Start-Process $easelExe }
}
if (-not $Yes) { Read-Host "Press Enter to close" | Out-Null }
if ($failures.Count -gt 0) { exit 2 }
