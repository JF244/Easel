#!/usr/bin/env python3
"""Turns detected hardware facts into an Easel setup profile.

Pure function: facts in, profile out. The Windows resolver
(installer/windows/EaselProfile.ps1) follows the same rules, and both must
pass every case in installer/profile-cases.json.

  python3 resolve_profile.py facts.json          # print the profile
  python3 resolve_profile.py --test               # run the shared cases
"""
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CATALOG = json.loads((HERE.parent / "catalog.json").read_text())

# Names that tell us the generation when nvidia-smi can't report compute capability.
LEGACY_NVIDIA_NAMES = re.compile(r"GTX\s*(9\d\d|10\d\d)|TITAN X|TITAN Xp|Quadro [MP]\d|Tesla [MP]\d", re.I)
MODERN_NVIDIA_NAMES = re.compile(r"RTX|GTX\s*16\d\d|TITAN RTX|TITAN V", re.I)


def pick_gpu(gpus):
    """The card that matters: the biggest NVIDIA or AMD card, else anything."""
    dedicated = [g for g in gpus if g.get("vendor") in ("nvidia", "amd")]
    pool = dedicated or gpus
    return max(pool, key=lambda g: g.get("vramGB") or 0) if pool else None


def compute_cap(gpu):
    if gpu.get("computeCap") is not None:
        return float(gpu["computeCap"])
    name = gpu.get("name", "")
    if LEGACY_NVIDIA_NAMES.search(name):
        return 6.1
    if MODERN_NVIDIA_NAMES.search(name):
        return 7.5
    return None


def resolve(facts):
    os_name = facts.get("os", "windows")
    ram = facts.get("ramGB", 0)
    gpu = pick_gpu(facts.get("gpus", []))
    vendor = gpu["vendor"] if gpu else "none"
    vram = float(gpu.get("vramGB") or 0) if gpu else 0.0
    cap = compute_cap(gpu) if gpu and vendor == "nvidia" else None
    warnings = []

    # 1. GPU -> compute backend
    if vendor == "nvidia" and vram >= 4:
        if cap is not None and cap < 5.0:
            backend = "cpu"
            warnings.append(f"This NVIDIA card (compute {cap:.1f}) is too old for current PyTorch; images use Gemini.")
        elif cap is not None and cap < 7.5:
            backend = "cuda-legacy"
            legacy = CATALOG["legacyNvidia"]
            warnings.append(
                f"Pascal-or-older NVIDIA card (compute {cap:.1f}): ComfyUI is pinned to {legacy['comfyTag']} with PyTorch {legacy['torch']}. "
                "Don't open the ComfyUI Desktop app on this PC: it reinstalls its own PyTorch and breaks this."
            )
        else:
            backend = "cuda"
    elif vendor == "amd" and os_name == "linux":
        backend = "rocm"
    elif vendor == "amd":
        backend = "cloud"
        warnings.append("AMD graphics on Windows: images use Gemini (cloud); the local brain runs on the GPU through llama.cpp (Vulkan).")
    else:
        backend = "cpu"
        warnings.append("No supported graphics card: images use Gemini, and the local brain runs on the CPU (slow).")

    # 2. VRAM -> checkpoint
    comfy = None
    checkpoint = None
    if backend in ("cuda", "cuda-legacy", "rocm"):
        if vram >= 16 and backend != "cuda-legacy":
            key = "flux-schnell"
        elif vram > 8:
            key = "sdxl"
        else:
            key = "dreamshaper-sd15"
        checkpoint = dict(CATALOG["checkpoints"][key], id=key)
        if backend == "cuda-legacy":
            legacy = CATALOG["legacyNvidia"]
            comfy = {"build": "nvidia", "pinTag": legacy["comfyTag"], "torch": legacy["torch"], "pipPackages": legacy["pipPackages"], "torchIndex": legacy["torchIndex"]}
        elif backend == "rocm":
            comfy = {"build": "linux-rocm", "pinTag": None, "torch": "rocm", "pipPackages": [], "torchIndex": None}
        else:
            comfy = {"build": "nvidia", "pinTag": None, "torch": "bundled", "pipPackages": [], "torchIndex": None}

    # 3. VRAM -> local brain model and context window
    gpu_for_llm = vram if backend != "cpu" else 0
    if gpu_for_llm >= 16:
        model, ctx = "qwen3:30b-a3b", 32768
    elif gpu_for_llm >= 12 and ram >= 32:
        model, ctx = "qwen3:30b-a3b", 16384  # MoE: experts that don't fit spill to system RAM
    elif gpu_for_llm >= 12:
        model, ctx = "qwen3:8b", 16384
        warnings.append("12-16 GB graphics memory with under 32 GB RAM: using qwen3:8b; qwen3:30b-a3b needs RAM to spill into.")
    elif gpu_for_llm > 0 or ram >= 16:
        model, ctx = "qwen3:8b", 8192
    else:
        model, ctx = "qwen3:4b", 4096

    servers = facts.get("servers", {})
    if servers.get("ollama"):
        runtime, url = "ollama", CATALOG["servers"]["ollama"]
    elif servers.get("lmstudio"):
        runtime, url = "lmstudio", CATALOG["servers"]["lmstudio"]
    elif backend == "cloud":
        runtime, url = "llamacpp-vulkan", CATALOG["servers"]["llamaServer"]
    else:
        runtime, url = "ollama", CATALOG["servers"]["ollama"]
    llm = {"runtime": runtime, "url": url, "model": model, "contextLength": ctx, "sizeGB": CATALOG["llms"][model]["sizeGB"]}
    if runtime == "llamacpp-vulkan":
        llm["ggufUrl"] = CATALOG["llms"][model]["ggufUrl"]

    if facts.get("freeGB", 1000) < 40:
        warnings.append(f"Only {facts.get('freeGB')} GB free; models can need 25 GB or more.")

    return {
        "summary": {
            "os": os_name,
            "gpuVendor": vendor,
            "gpuName": gpu.get("name", "") if gpu else "",
            "vramGB": vram,
            "computeCap": cap,
            "gpuBackend": backend,
        },
        "gpuBackend": backend,
        "imageEngine": "comfy" if comfy else "gemini",
        "comfy": comfy,
        "checkpoint": checkpoint,
        "llm": llm,
        "brain": {"order": ["local", "claudeCli"], "advisorOrder": ["claudeCli"], "advisorMode": "auto"},
        "warnings": warnings,
    }


def run_cases():
    cases = json.loads((HERE.parent / "profile-cases.json").read_text())["cases"]
    failed = 0
    for case in cases:
        got = resolve(case["facts"])
        problems = [f"{path}: expected {want!r}, got {lookup(got, path)!r}" for path, want in case["expect"].items() if lookup(got, path) != want]
        if problems:
            failed += 1
            print(f"FAIL {case['name']}")
            for p in problems:
                print(f"     {p}")
        else:
            print(f"ok   {case['name']}")
    print(f"{len(cases) - failed}/{len(cases)} cases pass")
    return failed == 0


def lookup(obj, path):
    for part in path.split("."):
        if obj is None:
            return None
        obj = obj.get(part) if isinstance(obj, dict) else None
    return obj


if __name__ == "__main__":
    if sys.argv[1:] == ["--test"]:
        sys.exit(0 if run_cases() else 1)
    source = open(sys.argv[1]) if len(sys.argv) > 1 and sys.argv[1] != "-" else sys.stdin
    print(json.dumps(resolve(json.load(source)), indent=2))
