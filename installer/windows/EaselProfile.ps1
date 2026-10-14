# Easel hardware profile for Windows: detect facts, resolve them into a setup
# profile. Dot-source this file; detect-hardware.ps1 and Easel-Setup.ps1 do.
#
# Resolve-EaselProfile follows exactly the same rules as
# installer/linux/resolve_profile.py, and both must pass every case in
# installer/profile-cases.json (Invoke-EaselProfileCases). Plain ASCII only, so
# Windows PowerShell 5.1 reads it correctly.

function Get-EaselCatalog {
  foreach ($dir in @($PSScriptRoot, (Split-Path -Parent $PSScriptRoot))) {
    $path = Join-Path $dir "catalog.json"
    if (Test-Path $path) { return Get-Content $path -Raw | ConvertFrom-Json }
  }
  throw "catalog.json not found next to the setup scripts"
}

function Get-EaselCasesPath {
  foreach ($dir in @($PSScriptRoot, (Split-Path -Parent $PSScriptRoot))) {
    $path = Join-Path $dir "profile-cases.json"
    if (Test-Path $path) { return $path }
  }
  return $null
}

# ---------------------------------------------------------------------------
# Detect
# ---------------------------------------------------------------------------

function Get-EaselGpus {
  $gpus = @()
  # The registry has the real memory size; Win32_VideoController caps it at 4 GB.
  $classKey = "HKLM:\SYSTEM\ControlSet001\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}"
  $registry = @{}
  Get-ChildItem $classKey -ErrorAction SilentlyContinue | ForEach-Object {
    $p = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
    if ($p -and $p.DriverDesc) {
      $bytes = $p."HardwareInformation.qwMemorySize"
      if (-not $bytes) { $bytes = $p."HardwareInformation.MemorySize" }
      if ($bytes -is [byte[]]) { $bytes = [BitConverter]::ToUInt64(($bytes + (New-Object byte[] 8))[0..7], 0) }
      if ($bytes) { $registry[[string]$p.DriverDesc] = [double]$bytes }
    }
  }
  foreach ($vc in @(Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue)) {
    $name = [string]$vc.Name
    if ($name -match "Microsoft Basic|Remote Display|Hyper-V|Parsec|Virtual") { continue }
    $bytes = $registry[$name]
    if (-not $bytes) { $bytes = [double]$vc.AdapterRAM }
    $vendor = "other"
    if ($name -match "NVIDIA") { $vendor = "nvidia" }
    elseif ($name -match "AMD|Radeon") { $vendor = "amd" }
    elseif ($name -match "Intel") { $vendor = "intel" }
    $gpus += [pscustomobject]@{ name = $name; vendor = $vendor; vramGB = [math]::Round($bytes / 1GB, 1); computeCap = $null }
  }
  # nvidia-smi knows NVIDIA memory exactly, and the compute capability (the Pascal test).
  if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) {
    try {
      foreach ($line in @(& nvidia-smi --query-gpu=name,memory.total,compute_cap --format=csv,noheader,nounits 2>$null)) {
        $parts = $line -split ","
        if ($parts.Count -lt 2) { continue }
        $gpu = $gpus | Where-Object { $_.name -eq $parts[0].Trim() } | Select-Object -First 1
        if (-not $gpu) {
          $gpu = [pscustomobject]@{ name = $parts[0].Trim(); vendor = "nvidia"; vramGB = 0; computeCap = $null }
          $gpus += $gpu
        }
        $gpu.vramGB = [math]::Round([double]$parts[1] / 1024, 1)
        if ($parts.Count -ge 3) {
          $cap = 0.0
          if ([double]::TryParse($parts[2].Trim(), [Globalization.NumberStyles]::Float, [Globalization.CultureInfo]::InvariantCulture, [ref]$cap)) { $gpu.computeCap = $cap }
        }
      }
    } catch { }
  }
  return ,$gpus
}

function Test-EaselServer([string]$Url) {
  try {
    $r = Invoke-WebRequest -Uri ($Url.TrimEnd("/") + "/v1/models") -UseBasicParsing -TimeoutSec 2
    return [int]$r.StatusCode -lt 400
  } catch { return $false }
}

function Find-EaselTool([string]$Name) {
  # npm installs put name.cmd on PATH; Easel resolves those itself now, so either counts.
  foreach ($candidate in @("$Name.exe", "$Name.cmd")) {
    $cmd = Get-Command $candidate -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
  }
  foreach ($path in @((Join-Path $env:USERPROFILE ".local\bin\$Name.exe"), (Join-Path $env:APPDATA "npm\$Name.cmd"))) {
    if (Test-Path $path) { return $path }
  }
  return $null
}

function Get-EaselHardwareFacts([string]$InstallDir = $env:LOCALAPPDATA) {
  $catalog = Get-EaselCatalog
  $cs = Get-CimInstance Win32_ComputerSystem
  $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
  $drive = (Split-Path -Qualifier $InstallDir).TrimEnd(":")
  $servers = [ordered]@{}
  foreach ($p in $catalog.servers.PSObject.Properties) { $servers[$p.Name] = Test-EaselServer $p.Value }
  $tools = [ordered]@{}
  foreach ($t in @("claude", "agy", "ollama", "nvidia-smi")) { $tools[$t] = Find-EaselTool $t }
  [pscustomobject][ordered]@{
    os = "windows"
    arch = $(if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x64" })
    ramGB = [math]::Round($cs.TotalPhysicalMemory / 1GB)
    cpuName = [string]$cpu.Name
    cpuCores = [int]$cpu.NumberOfLogicalProcessors
    freeGB = [math]::Round((Get-PSDrive -Name $drive).Free / 1GB)
    gpus = (Get-EaselGpus)
    servers = [pscustomobject]$servers
    tools = [pscustomobject]$tools
  }
}

# ---------------------------------------------------------------------------
# Resolve (pure: the same rules as resolve_profile.py)
# ---------------------------------------------------------------------------

function Get-EaselComputeCap($Gpu) {
  if ($null -ne $Gpu.computeCap) { return [double]$Gpu.computeCap }
  if ($Gpu.name -match "GTX\s*(9\d\d|10\d\d)|TITAN X|TITAN Xp|Quadro [MP]\d|Tesla [MP]\d") { return 6.1 }
  if ($Gpu.name -match "RTX|GTX\s*16\d\d|TITAN RTX|TITAN V") { return 7.5 }
  return $null
}

function Resolve-EaselProfile($Facts, $Catalog = $(Get-EaselCatalog)) {
  $os = if ($Facts.os) { [string]$Facts.os } else { "windows" }
  $ram = [double]$Facts.ramGB
  $all = @($Facts.gpus)
  $pool = @($all | Where-Object { $_.vendor -eq "nvidia" -or $_.vendor -eq "amd" })
  if ($pool.Count -eq 0) { $pool = $all }
  $gpu = $pool | Sort-Object { [double]$_.vramGB } -Descending | Select-Object -First 1
  $vendor = if ($gpu) { [string]$gpu.vendor } else { "none" }
  $vram = if ($gpu -and $gpu.vramGB) { [double]$gpu.vramGB } else { 0.0 }
  $cap = if ($gpu -and $vendor -eq "nvidia") { Get-EaselComputeCap $gpu } else { $null }
  $warnings = New-Object System.Collections.ArrayList
  $legacy = $Catalog.legacyNvidia

  # 1. GPU -> compute backend
  if ($vendor -eq "nvidia" -and $vram -ge 4) {
    if ($null -ne $cap -and $cap -lt 5.0) {
      $backend = "cpu"
      [void]$warnings.Add(("This NVIDIA card (compute {0:N1}) is too old for current PyTorch; images use Gemini." -f $cap))
    } elseif ($null -ne $cap -and $cap -lt 7.5) {
      $backend = "cuda-legacy"
      [void]$warnings.Add(("Pascal-or-older NVIDIA card (compute {0:N1}): ComfyUI is pinned to {1} with PyTorch {2}. Don't open the ComfyUI Desktop app on this PC: it reinstalls its own PyTorch and breaks this." -f $cap, $legacy.comfyTag, $legacy.torch))
    } else {
      $backend = "cuda"
    }
  } elseif ($vendor -eq "amd" -and $os -eq "linux") {
    $backend = "rocm"
  } elseif ($vendor -eq "amd") {
    $backend = "cloud"
    [void]$warnings.Add("AMD graphics on Windows: images use Gemini (cloud); the local brain runs on the GPU through llama.cpp (Vulkan).")
  } else {
    $backend = "cpu"
    [void]$warnings.Add("No supported graphics card: images use Gemini, and the local brain runs on the CPU (slow).")
  }

  # 2. VRAM -> checkpoint
  $comfy = $null
  $checkpoint = $null
  if (@("cuda", "cuda-legacy", "rocm") -contains $backend) {
    if ($vram -ge 16 -and $backend -ne "cuda-legacy") { $key = "flux-schnell" }
    elseif ($vram -gt 8) { $key = "sdxl" }
    else { $key = "dreamshaper-sd15" }
    $checkpoint = $Catalog.checkpoints.$key | Select-Object *
    $checkpoint | Add-Member -NotePropertyName id -NotePropertyValue $key
    if ($backend -eq "cuda-legacy") {
      $comfy = [pscustomobject][ordered]@{ build = "nvidia"; pinTag = $legacy.comfyTag; torch = $legacy.torch; pipPackages = @($legacy.pipPackages); torchIndex = $legacy.torchIndex }
    } elseif ($backend -eq "rocm") {
      $comfy = [pscustomobject][ordered]@{ build = "linux-rocm"; pinTag = $null; torch = "rocm"; pipPackages = @(); torchIndex = $null }
    } else {
      $comfy = [pscustomobject][ordered]@{ build = "nvidia"; pinTag = $null; torch = "bundled"; pipPackages = @(); torchIndex = $null }
    }
  }

  # 3. VRAM -> local brain model and context window
  $gpuForLlm = if ($backend -ne "cpu") { $vram } else { 0 }
  if ($gpuForLlm -ge 16) { $model = "qwen3:30b-a3b"; $ctx = 32768 }
  elseif ($gpuForLlm -ge 12 -and $ram -ge 32) { $model = "qwen3:30b-a3b"; $ctx = 16384 }
  elseif ($gpuForLlm -ge 12) {
    $model = "qwen3:8b"; $ctx = 16384
    [void]$warnings.Add("12-16 GB graphics memory with under 32 GB RAM: using qwen3:8b; qwen3:30b-a3b needs RAM to spill into.")
  }
  elseif ($gpuForLlm -gt 0 -or $ram -ge 16) { $model = "qwen3:8b"; $ctx = 8192 }
  else { $model = "qwen3:4b"; $ctx = 4096 }

  $servers = $Facts.servers
  if ($servers -and $servers.ollama) { $runtime = "ollama"; $url = $Catalog.servers.ollama }
  elseif ($servers -and $servers.lmstudio) { $runtime = "lmstudio"; $url = $Catalog.servers.lmstudio }
  elseif ($backend -eq "cloud") { $runtime = "llamacpp-vulkan"; $url = $Catalog.servers.llamaServer }
  else { $runtime = "ollama"; $url = $Catalog.servers.ollama }
  $llm = [ordered]@{ runtime = $runtime; url = $url; model = $model; contextLength = $ctx; sizeGB = $Catalog.llms.$model.sizeGB }
  if ($runtime -eq "llamacpp-vulkan") { $llm.ggufUrl = $Catalog.llms.$model.ggufUrl }

  if ($null -ne $Facts.freeGB -and [double]$Facts.freeGB -lt 40) {
    [void]$warnings.Add(("Only {0} GB free; models can need 25 GB or more." -f $Facts.freeGB))
  }

  $summary = [pscustomobject][ordered]@{
    os = $os; gpuVendor = $vendor; gpuName = $(if ($gpu) { [string]$gpu.name } else { "" })
    vramGB = $vram; computeCap = $cap; gpuBackend = $backend
  }
  [pscustomobject][ordered]@{
    summary = $summary
    gpuBackend = $backend
    imageEngine = $(if ($comfy) { "comfy" } else { "gemini" })
    comfy = $comfy
    checkpoint = $checkpoint
    llm = [pscustomobject]$llm
    brain = [pscustomobject][ordered]@{ order = @("local", "claudeCli"); advisorOrder = @("claudeCli"); advisorMode = "auto" }
    warnings = @($warnings)
  }
}

# ---------------------------------------------------------------------------
# Shared test cases
# ---------------------------------------------------------------------------

function Get-EaselPathValue($Object, [string]$Path) {
  foreach ($part in $Path.Split(".")) {
    if ($null -eq $Object) { return $null }
    $prop = $Object.PSObject.Properties[$part]
    if (-not $prop) { return $null }
    $Object = $prop.Value
  }
  return $Object
}

function Invoke-EaselProfileCases {
  $path = Get-EaselCasesPath
  if (-not $path) { throw "profile-cases.json not found" }
  $catalog = Get-EaselCatalog
  $cases = (Get-Content $path -Raw | ConvertFrom-Json).cases
  $failed = 0
  foreach ($case in $cases) {
    $got = Resolve-EaselProfile $case.facts $catalog
    $problems = @()
    foreach ($p in $case.expect.PSObject.Properties) {
      $want = $p.Value
      $have = Get-EaselPathValue $got $p.Name
      $same = if ($null -eq $want) { $null -eq $have } elseif ($want -is [double] -or $want -is [int] -or $want -is [long] -or $want -is [decimal]) { $null -ne $have -and [math]::Abs([double]$have - [double]$want) -lt 1e-9 } else { [string]$have -ceq [string]$want }
      if (-not $same) { $problems += ("{0}: expected '{1}', got '{2}'" -f $p.Name, $want, $have) }
    }
    if ($problems.Count) { $failed++; Write-Host "FAIL $($case.name)"; $problems | ForEach-Object { Write-Host "     $_" } }
    else { Write-Host "ok   $($case.name)" }
  }
  Write-Host ("{0}/{1} cases pass" -f ($cases.Count - $failed), $cases.Count)
  return ($failed -eq 0)
}
