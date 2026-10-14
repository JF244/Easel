#!/usr/bin/env python3
"""Looks at this Linux PC and prints Easel's setup profile as JSON.

  python3 detect_hardware.py                 # print { "detected": ..., "resolved": ..., "summary": ... }
  python3 detect_hardware.py --facts         # only what was detected
  python3 detect_hardware.py --write         # also save ~/.local/share/easel-suite/hardware-profile.json

Easel reads the saved profile to warn before ComfyUI jobs that can't work on
this GPU. Needs only Python 3; uses nvidia-smi and lspci when they exist.
"""
import json
import os
import platform
import shutil
import subprocess
import sys
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from resolve_profile import CATALOG, resolve  # noqa: E402

PCI_VENDORS = {"0x10de": "nvidia", "0x1002": "amd", "0x8086": "intel"}


def run(cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=10).stdout
    except (OSError, subprocess.SubprocessError):
        return ""


def nvidia_gpus():
    if not shutil.which("nvidia-smi"):
        return []
    gpus = []
    out = run(["nvidia-smi", "--query-gpu=name,memory.total,compute_cap", "--format=csv,noheader,nounits"])
    for line in out.splitlines():
        parts = [p.strip() for p in line.split(",")]
        if len(parts) >= 2:
            cap = None
            try:
                cap = float(parts[2])
            except (IndexError, ValueError):
                pass
            gpus.append({"name": parts[0], "vendor": "nvidia", "vramGB": round(float(parts[1]) / 1024, 1), "computeCap": cap})
    return gpus


def drm_gpus():
    """AMD and Intel cards from sysfs; amdgpu reports its memory there."""
    gpus = []
    for card in sorted(Path("/sys/class/drm").glob("card[0-9]*")):
        if "-" in card.name:  # card0-DP-1 and friends are outputs, not cards
            continue
        device = card / "device"
        try:
            vendor = PCI_VENDORS.get((device / "vendor").read_text().strip(), "other")
        except OSError:
            continue
        if vendor == "nvidia":
            continue  # nvidia-smi already covered these, with better numbers
        vram = 0.0
        try:
            vram = round(int((device / "mem_info_vram_total").read_text()) / 2**30, 1)
        except (OSError, ValueError):
            pass
        slot = os.path.basename(os.path.realpath(device))
        name = run(["lspci", "-s", slot]).split(": ", 1)[-1].strip() if shutil.which("lspci") else ""
        gpus.append({"name": name or f"{vendor} graphics", "vendor": vendor, "vramGB": vram, "computeCap": None})
    return gpus


def server_up(url):
    try:
        with urllib.request.urlopen(url.rstrip("/") + "/v1/models", timeout=1.5) as r:
            return r.status < 400
    except Exception:
        return False


def facts():
    mem_kb = 0
    for line in Path("/proc/meminfo").read_text().splitlines():
        if line.startswith("MemTotal:"):
            mem_kb = int(line.split()[1])
    free = shutil.disk_usage(Path.home())
    servers = CATALOG["servers"]
    return {
        "os": "linux",
        "arch": "arm64" if platform.machine() in ("aarch64", "arm64") else "x64",
        "ramGB": round(mem_kb / 2**20),
        "cpuName": next((l.split(":", 1)[1].strip() for l in Path("/proc/cpuinfo").read_text().splitlines() if l.startswith("model name")), ""),
        "cpuCores": os.cpu_count() or 0,
        "freeGB": round(free.free / 2**30),
        "gpus": nvidia_gpus() + drm_gpus(),
        "servers": {name: server_up(url) for name, url in servers.items()},
        "tools": {t: shutil.which(t) for t in ("claude", "agy", "ollama", "nvidia-smi", "rocminfo")},
    }


def main():
    detected = facts()
    if "--facts" in sys.argv:
        print(json.dumps(detected, indent=2))
        return
    resolved = resolve(detected)
    profile = {"detected": detected, "resolved": resolved, "summary": resolved["summary"]}
    text = json.dumps(profile, indent=2)
    if "--write" in sys.argv:
        out = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")) / "easel-suite" / "hardware-profile.json"
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(text)
        print(f"Saved {out}", file=sys.stderr)
    print(text)


if __name__ == "__main__":
    main()
