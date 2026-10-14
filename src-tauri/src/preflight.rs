//! Checks, before a ComfyUI job, whether this GPU and ComfyUI's PyTorch can
//! actually work together, so you get a plain warning instead of a job that
//! dies with "cudaErrorNoKernelImageForDevice" deep inside ComfyUI.
//!
//! Sources: ComfyUI's own /system_stats (device, PyTorch and ComfyUI
//! versions), nvidia-smi (compute capability), and the hardware profile the
//! setup writes (for AMD on Windows and other cases nvidia-smi can't see).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// Pascal and older (compute < 7.5) need these; newer PyTorch builds dropped them.
pub const LEGACY_TORCH: &str = "2.6.0+cu124";
pub const LEGACY_COMFY_MAX: &str = "0.7.0";

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preflight {
  /// Problems that will make ComfyUI jobs fail or crawl, in plain words.
  pub warnings: Vec<String>,
  /// What ComfyUI reported, for showing next to the warning.
  pub device: Option<String>,
  pub torch: Option<String>,
  pub comfy_version: Option<String>,
}

/// The part of the setup's hardware profile Easel cares about.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProfileHints {
  pub os: String,
  pub gpu_vendor: String,
  pub gpu_name: String,
  pub compute_cap: Option<f64>,
  pub gpu_backend: String,
}

pub async fn check(client: &reqwest::Client, comfy_url: &str, profile_path: Option<&Path>) -> Preflight {
  let hints = profile_path.and_then(read_profile).unwrap_or_default();
  let compute_cap = hints.compute_cap.or_else(nvidia_compute_cap);
  let stats = fetch_stats(client, comfy_url).await;
  evaluate(&hints, compute_cap, stats.as_ref())
}

/// The rules, separate from the probing so they can be tested.
pub fn evaluate(hints: &ProfileHints, compute_cap: Option<f64>, stats: Option<&Value>) -> Preflight {
  let mut out = Preflight::default();
  let windows = hints.os == "windows" || (hints.os.is_empty() && cfg!(windows));

  if let Some(stats) = stats {
    out.comfy_version = stats.pointer("/system/comfyui_version").and_then(Value::as_str).map(str::to_string);
    out.torch = stats.pointer("/system/pytorch_version").and_then(Value::as_str).map(str::to_string);
    let device = stats.pointer("/devices/0");
    out.device = device.and_then(|d| d.get("name")).and_then(Value::as_str).map(|n| n.trim().to_string());
    if device.and_then(|d| d.get("type")).and_then(Value::as_str) == Some("cpu") {
      out.warnings.push(
        "ComfyUI is running on the CPU, so each image can take many minutes. If you have a supported graphics card, start ComfyUI with its GPU launcher; otherwise Gemini is the faster choice.".into(),
      );
    }
  }

  if let Some(cap) = compute_cap.filter(|c| *c < 7.5) {
    if cap < 5.0 {
      out.warnings.push(format!(
        "This NVIDIA card (compute {cap:.1}) is too old for any current PyTorch, so ComfyUI can only use the CPU. Use Gemini for images."
      ));
    } else {
      let torch_ok = out.torch.as_deref().map(torch_supports_legacy).unwrap_or(true);
      let comfy_ok = out.comfy_version.as_deref().map(|v| version_at_most(v, LEGACY_COMFY_MAX)).unwrap_or(true);
      if !torch_ok || !comfy_ok {
        out.warnings.push(format!(
          "This NVIDIA card (compute {cap:.1}, Pascal or older) needs PyTorch {LEGACY_TORCH} and ComfyUI {LEGACY_COMFY_MAX} or older. ComfyUI here has {} and PyTorch {}, so jobs will fail with \"no kernel image\". Re-run Easel's setup to install the matching versions, and don't open the ComfyUI Desktop app afterwards: it reinstalls its own PyTorch.",
          out.comfy_version.as_deref().unwrap_or("an unknown version"),
          out.torch.as_deref().unwrap_or("unknown"),
        ));
      }
    }
  }

  if windows && hints.gpu_vendor == "amd" && hints.gpu_backend != "rocm" && hints.gpu_backend != "amd" {
    out.warnings.push(format!(
      "AMD graphics on Windows ({}) isn't reliable with ComfyUI. Gemini is the better image engine here; local brains still run on your GPU through llama.cpp (Vulkan).",
      if hints.gpu_name.is_empty() { "this card" } else { &hints.gpu_name }
    ));
  }
  out
}

async fn fetch_stats(client: &reqwest::Client, comfy_url: &str) -> Option<Value> {
  let base = comfy_url.trim().trim_end_matches('/');
  if base.is_empty() {
    return None;
  }
  let resp = client.get(format!("{base}/system_stats")).timeout(Duration::from_secs(3)).send().await.ok()?;
  resp.json().await.ok()
}

fn read_profile(path: &Path) -> Option<ProfileHints> {
  let text = std::fs::read_to_string(path).ok()?;
  let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
  // The setup writes { detected: {...}, resolved: {...} }; the hints live in "summary".
  serde_json::from_value(value.get("summary").cloned().unwrap_or(value)).ok()
}

/// Highest compute capability nvidia-smi reports, if it's installed.
fn nvidia_compute_cap() -> Option<f64> {
  let mut command = std::process::Command::new("nvidia-smi");
  command.args(["--query-gpu=compute_cap", "--format=csv,noheader"]);
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
  }
  let output = command.output().ok()?;
  String::from_utf8_lossy(&output.stdout)
    .lines()
    .filter_map(|l| l.trim().parse::<f64>().ok())
    .fold(None, |best: Option<f64>, c| Some(best.map_or(c, |b| b.max(c))))
}

/// PyTorch 2.6 and older still ship kernels for Pascal (sm_50-61) in their CUDA 12.4 builds.
fn torch_supports_legacy(version: &str) -> bool {
  let core = version.split('+').next().unwrap_or(version);
  version_at_most(core, "2.6.99")
}

/// Compares dotted versions ("0.3.75" vs "0.7.0"), ignoring a leading "v".
fn version_at_most(version: &str, max: &str) -> bool {
  let parse = |v: &str| -> Vec<u64> {
    v.trim().trim_start_matches('v').split(['.', '-']).map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)).collect()
  };
  let (a, b) = (parse(version), parse(max));
  for i in 0..a.len().max(b.len()) {
    let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
    if x != y {
      return x < y;
    }
  }
  true
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn pascal_with_a_new_comfyui_gets_an_actionable_warning() {
    let stats = stats("cuda", "NVIDIA GeForce GTX 1080", "2.12.0+cu130", "0.9.2");
    let p = evaluate(&ProfileHints::default(), Some(6.1), Some(&stats));
    assert_eq!(p.warnings.len(), 1);
    assert!(p.warnings[0].contains("2.6.0+cu124"));
    assert!(p.warnings[0].contains("don't open the ComfyUI Desktop app"));
  }

  #[test]
  fn pascal_on_the_pinned_versions_is_fine() {
    let stats = stats("cuda", "NVIDIA GeForce GTX 1080", "2.6.0+cu124", "0.7.0");
    assert!(evaluate(&ProfileHints::default(), Some(6.1), Some(&stats)).warnings.is_empty());
  }

  #[test]
  fn modern_cards_and_unknown_cards_raise_nothing() {
    let stats = stats("cuda", "NVIDIA GeForce RTX 4070", "2.12.0+cu130", "0.9.2");
    assert!(evaluate(&ProfileHints::default(), Some(8.9), Some(&stats)).warnings.is_empty());
    assert!(evaluate(&ProfileHints::default(), None, None).warnings.is_empty());
  }

  #[test]
  fn cpu_only_comfyui_is_called_out() {
    let stats = stats("cpu", "cpu", "2.12.0+cpu", "0.9.2");
    let p = evaluate(&ProfileHints::default(), None, Some(&stats));
    assert!(p.warnings[0].starts_with("ComfyUI is running on the CPU"));
  }

  #[test]
  fn amd_on_windows_points_to_gemini() {
    let hints = ProfileHints { os: "windows".into(), gpu_vendor: "amd".into(), gpu_name: "Radeon RX 6700 XT".into(), gpu_backend: "cloud".into(), ..Default::default() };
    let p = evaluate(&hints, None, None);
    assert!(p.warnings[0].contains("Gemini is the better image engine"));
  }

  #[test]
  fn very_old_nvidia_is_cpu_only() {
    assert!(evaluate(&ProfileHints::default(), Some(3.5), None).warnings[0].contains("too old"));
  }

  #[test]
  fn version_comparison() {
    assert!(version_at_most("0.3.75", "0.7.0"));
    assert!(version_at_most("v0.7.0", "0.7.0"));
    assert!(!version_at_most("0.7.1", "0.7.0"));
    assert!(torch_supports_legacy("2.6.0+cu124"));
    assert!(!torch_supports_legacy("2.7.1+cu128"));
  }

  #[test]
  fn reads_the_setup_profile_summary() {
    let dir = std::env::temp_dir().join(format!("easel-pf-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hardware-profile.json");
    std::fs::write(&path, "\u{feff}{\"summary\":{\"os\":\"windows\",\"gpuVendor\":\"nvidia\",\"computeCap\":6.1,\"gpuBackend\":\"cuda-legacy\"}}").unwrap();
    let hints = read_profile(&path).unwrap();
    assert_eq!(hints.compute_cap, Some(6.1));
    assert_eq!(hints.gpu_backend, "cuda-legacy");
    let _ = std::fs::remove_dir_all(dir);
  }

  fn stats(kind: &str, name: &str, torch: &str, comfy: &str) -> Value {
    json!({ "system": { "pytorch_version": torch, "comfyui_version": comfy }, "devices": [{ "name": name, "type": kind }] })
  }
}
