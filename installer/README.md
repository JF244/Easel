# Easel setup

The setup is a config resolver: **detect** the hardware, **resolve** it into a
profile, then **install** and write Easel's settings from that profile.

```
detect-hardware  ->  facts (GPU, VRAM, compute capability, RAM, servers, tools)
resolve          ->  profile (backend, ComfyUI pin, torch, checkpoint, LLM, warnings)
Easel-Setup      ->  installs from the profile, writes settings.json and hardware-profile.json
```

| File                               | What it is                                                                |
|------------------------------------|---------------------------------------------------------------------------|
| `catalog.json`                     | Model tiers, download links, per-checkpoint settings. One source for both |
| `profile-cases.json`               | Test machines and the profile each must produce                           |
| `windows/detect-hardware.ps1`      | Prints this PC's profile; `-Save` stores it for Easel; `-Test` runs cases |
| `windows/EaselProfile.ps1`         | Windows detection and the resolver (plain ASCII, PowerShell 5.1)          |
| `windows/Easel-Setup.ps1`          | The installer; `Setup Easel.cmd` runs it with a double-click              |
| `linux/detect_hardware.py`         | Prints this PC's profile; `--write` stores it for Easel                   |
| `linux/resolve_profile.py`         | The resolver in Python; `--test` runs the shared cases                    |

Both resolvers must pass every case in `profile-cases.json`, so they can't
drift apart. CI runs them on Windows PowerShell 5.1 and on Linux.

## Try it on a PC first

On Windows, without installing anything:

```powershell
powershell -ExecutionPolicy Bypass -File detect-hardware.ps1
powershell -ExecutionPolicy Bypass -File Easel-Setup.ps1 -DryRun
```

On Linux: `python3 linux/detect_hardware.py`.

## The decision matrix

GPU to compute backend:

| Detected                               | Backend       | What gets installed                                                       |
|----------------------------------------|---------------|---------------------------------------------------------------------------|
| NVIDIA, compute 7.5 or newer           | `cuda`        | Latest ComfyUI portable with its bundled PyTorch                          |
| NVIDIA Pascal or older (compute 5.0-7.4) | `cuda-legacy` | ComfyUI **v0.7.0**, then PyTorch **2.6.0+cu124** inside its Python      |
| NVIDIA older than compute 5.0          | `cpu`         | No ComfyUI; Gemini for images                                             |
| AMD on Linux                           | `rocm`        | ComfyUI on ROCm PyTorch (Linux installer not written yet)                 |
| AMD on Windows                         | `cloud`       | No ComfyUI; Gemini for images; llama.cpp **Vulkan** for the local brain   |
| Intel, none, or under 4 GB             | `cpu`         | No ComfyUI; Gemini for images; local brain on the CPU                     |

When `nvidia-smi` can't report compute capability, the card's name decides
(GTX 9xx/10xx and Pascal Quadro/Tesla count as legacy; RTX and GTX 16xx don't).

Graphics memory to models:

| Graphics memory         | Local brain                               | Checkpoint            | Model size |
|-------------------------|-------------------------------------------|-----------------------|------------|
| 8 GB or less            | `qwen3:8b`, 8K context                    | DreamShaper 8 (SD1.5) | 512        |
| 9-11 GB                 | `qwen3:8b`, 8K context                    | SDXL                  | 1024       |
| 12-15 GB, 32 GB+ RAM    | `qwen3:30b-a3b` (spills to RAM), 16K      | SDXL                  | 1024       |
| 12-15 GB, less RAM      | `qwen3:8b`, 16K context                   | SDXL                  | 1024       |
| 16 GB or more           | `qwen3:30b-a3b` in graphics memory, 32K   | FLUX.1 schnell        | 1024       |
| No usable GPU           | `qwen3:8b` (16 GB+ RAM) or `qwen3:4b`      | none (Gemini)         |            |

Pascal cards top out at SDXL: they can't run Flux's fp8 weights usefully.

The local brain's runtime: an Ollama already running wins, then an LM Studio
already running; otherwise Ollama is installed (llama.cpp Vulkan on AMD +
Windows). The context window is set with `OLLAMA_CONTEXT_LENGTH` (Ollama) or
`-c` (llama-server). Steps, CFG, sampler and scheduler come from the
checkpoint's entry in `catalog.json`.

## What a Windows + GTX 1080 test run taught us

These are why the matrix looks the way it does.

- **Pascal needs old versions.** PyTorch builds newer than 2.6 dropped
  compute 5.0-6.1, and ComfyUI releases after 2026-01-05 need `comfy_kitchen`,
  which needs PyTorch 2.12+. A newer ComfyUI fails deep inside with
  `cudaErrorNoKernelImageForDevice`. The setup pins ComfyUI v0.7.0 and installs
  `torch==2.6.0`, `torchvision==0.21.0`, `torchaudio==2.6.0` from the cu124
  index into ComfyUI's own Python, then checks the version.
- **Don't open the ComfyUI Desktop app after that.** It reinstalls its bundled
  PyTorch (2.12+cu130), which silently drops these GPUs again. The setup uses
  the standalone portable ComfyUI for that reason.
- **AMD on Windows: don't fight it.** Images go to Gemini; the local brain
  runs on the GPU through llama.cpp's Vulkan build.
- **npm installs Claude Code as `claude.cmd`.** Easel now reads the wrapper and
  starts the real program (never through cmd.exe, which mangles long prompts).
- **Ollama listens on 11434, LM Studio on 1234.** Easel now tries 11434, 1234
  and 8080 and uses whichever answers; Settings has "Detect running servers".
- **Unsupported GPUs used to fail cryptically.** Easel now checks ComfyUI's
  `/system_stats` against the GPU's compute capability (and this profile) and
  warns before the job.

## Options

`-DryRun`, `-DryRun -CheckLinks`, `-InstallDir D:\Easel`, `-SkipModels`,
`-SkipApps`, `-SkipComfy`, `-SkipLocalBrain`, `-SkipClaude`, `-NoAutostart`,
and `-FactsFile case.json` to plan (or install) as if this were another PC.
The log is `%LOCALAPPDATA%\EaselSuite\setup.log`; the profile Easel reads is
`%LOCALAPPDATA%\EaselSuite\hardware-profile.json`.
