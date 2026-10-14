//! Settings live in one JSON file in the app's config folder. API keys are
//! stored there too: this is a personal, single-user app, so the file is
//! readable only by you (0600 on Linux) and never leaves your PC.

use crate::apps::AppPaths;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_GEMINI_IMAGE_MODEL: &str = "gemini-2.5-flash-image";
pub const DEFAULT_CLAUDE_MODEL: &str = "claude-opus-5-5";
pub const DEFAULT_COMFY_URL: &str = "http://127.0.0.1:8188";
/// Ollama's port. If nothing answers there, Easel also tries LM Studio (1234) and llama-server (8080).
pub const DEFAULT_LOCAL_LLM_URL: &str = "http://127.0.0.1:11434";
pub const DEFAULT_CLAUDE_CLI: &str = "claude";
pub const DEFAULT_AGY_CLI: &str = "agy";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
  /// Where generated images are saved. Empty means Pictures/Easel.
  pub output_dir: String,
  pub gemini: GeminiSettings,
  pub comfy: ComfySettings,
  pub brain: BrainSettings,
  /// Any other editor, for files no Craft app handles.
  pub editor_path: String,
  /// Hand-set paths for the Craft apps, keyed by app id ("photocraft").
  pub apps: HashMap<String, AppPaths>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GeminiSettings {
  pub api_key: String,
  pub image_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ComfySettings {
  pub url: String,
  pub checkpoint: String,
  pub steps: u32,
  pub cfg: f32,
  pub sampler: String,
  pub scheduler: String,
  pub negative: String,
  /// Long side of a square image in pixels: 1024 for SDXL and Flux, 512 for SD 1.5.
  pub base_size: u32,
  pub workflows: Vec<CustomWorkflow>,
}

/// A workflow exported from ComfyUI with "Save (API Format)". Text values like
/// "{{prompt}}" or "{{seed}}" are filled in when it runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomWorkflow {
  pub id: String,
  pub name: String,
  pub json: String,
}

/// The "brain" plans jobs and writes prompts. Easel tries each brain in
/// `order` and moves to the next when one fails or is out of usage.
/// Stored as plain names ("claudeCli", "remote:lan-box") so remote servers
/// can be added freely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum BrainKind {
  /// Claude Code in print mode (`claude -p`): uses your Claude subscription.
  ClaudeCli,
  /// Google Antigravity CLI in print mode (`agy -p`): uses your Google subscription.
  AgyCli,
  /// The Anthropic API with your own key: pay per use.
  ClaudeApi,
  /// Gemini (for example Gemini 4) with the same API key as the image models.
  Gemini,
  /// LM Studio, Ollama or any OpenAI-compatible server on your PC.
  Local,
  /// One of your remote servers, by its id.
  Remote(String),
}

/// Any OpenAI-compatible endpoint on another machine or online: llama.cpp's
/// llama-server (including one pooling GPUs with --rpc), vLLM, LM Studio or
/// Ollama on another PC, or a hosted API.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RemoteBrain {
  pub id: String,
  pub name: String,
  pub url: String,
  pub api_key: String,
  pub model: String,
}

/// When the advisor (a stronger, frontier model) is asked.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AdvisorMode {
  /// Never.
  Off,
  /// Only when you start a message with /advisor.
  OnRequest,
  /// Also when the primary brain says it's unsure.
  Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BrainSettings {
  #[serde(deserialize_with = "lenient_brains")]
  pub order: Vec<BrainKind>,
  /// Frontier brains asked for advice, tried in this order.
  #[serde(deserialize_with = "lenient_brains")]
  pub advisor_order: Vec<BrainKind>,
  pub advisor_mode: AdvisorMode,
  pub remotes: Vec<RemoteBrain>,
  pub claude_cli_path: String,
  /// Optional model for Claude Code, e.g. "opus" or "sonnet". Empty uses its default.
  pub claude_cli_model: String,
  pub agy_cli_path: String,
  pub claude_api_key: String,
  pub claude_api_model: String,
  /// Gemini text model for the Gemini brain. Empty picks the newest Pro model your key lists.
  pub gemini_model: String,
  /// Filled in at run time from the Gemini section, so the key is stored once.
  #[serde(skip)]
  pub gemini_api_key: String,
  /// Any OpenAI-compatible server: LM Studio (port 1234) or Ollama (port 11434).
  pub local_url: String,
  pub local_model: String,
}

impl Default for Settings {
  fn default() -> Self {
    Self {
      output_dir: String::new(),
      gemini: GeminiSettings::default(),
      comfy: ComfySettings::default(),
      brain: BrainSettings::default(),
      editor_path: String::new(),
      apps: HashMap::new(),
    }
  }
}

impl Default for GeminiSettings {
  fn default() -> Self {
    Self { api_key: String::new(), image_model: DEFAULT_GEMINI_IMAGE_MODEL.into() }
  }
}

impl Default for ComfySettings {
  fn default() -> Self {
    Self {
      url: DEFAULT_COMFY_URL.into(),
      checkpoint: String::new(),
      steps: 28,
      cfg: 6.0,
      sampler: "euler".into(),
      scheduler: "normal".into(),
      negative: "blurry, low quality, watermark, text artifacts".into(),
      base_size: 1024,
      workflows: Vec::new(),
    }
  }
}

impl Default for BrainSettings {
  fn default() -> Self {
    Self {
      order: vec![BrainKind::ClaudeCli, BrainKind::Local],
      advisor_order: vec![BrainKind::ClaudeCli],
      advisor_mode: AdvisorMode::OnRequest,
      remotes: Vec::new(),
      claude_cli_path: DEFAULT_CLAUDE_CLI.into(),
      claude_cli_model: String::new(),
      agy_cli_path: DEFAULT_AGY_CLI.into(),
      claude_api_key: String::new(),
      claude_api_model: DEFAULT_CLAUDE_MODEL.into(),
      gemini_model: String::new(),
      gemini_api_key: String::new(),
      local_url: DEFAULT_LOCAL_LLM_URL.into(),
      local_model: String::new(),
    }
  }
}

impl TryFrom<String> for BrainKind {
  type Error = String;

  fn try_from(name: String) -> Result<Self, Self::Error> {
    match name.as_str() {
      "claudeCli" => Ok(BrainKind::ClaudeCli),
      "agyCli" => Ok(BrainKind::AgyCli),
      "claudeApi" => Ok(BrainKind::ClaudeApi),
      "gemini" => Ok(BrainKind::Gemini),
      "local" => Ok(BrainKind::Local),
      other => match other.strip_prefix("remote:") {
        Some(id) if !id.is_empty() => Ok(BrainKind::Remote(id.to_string())),
        _ => Err(format!("unknown brain \"{other}\"")),
      },
    }
  }
}

impl From<BrainKind> for String {
  fn from(kind: BrainKind) -> String {
    match kind {
      BrainKind::ClaudeCli => "claudeCli".into(),
      BrainKind::AgyCli => "agyCli".into(),
      BrainKind::ClaudeApi => "claudeApi".into(),
      BrainKind::Gemini => "gemini".into(),
      BrainKind::Local => "local".into(),
      BrainKind::Remote(id) => format!("remote:{id}"),
    }
  }
}

/// Skips brains this version doesn't know instead of throwing away all settings.
fn lenient_brains<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<BrainKind>, D::Error> {
  let names: Vec<String> = Deserialize::deserialize(deserializer)?;
  Ok(names.into_iter().filter_map(|n| BrainKind::try_from(n).ok()).collect())
}

impl Settings {
  pub fn load(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
      Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
        log::warn!("settings file is not valid JSON, using defaults: {err}");
        Settings::default()
      }),
      Err(_) => Settings::default(),
    }
  }

  pub fn save(&self, path: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent() {
      std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create the settings folder: {e}"))?;
    }
    let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
    // Write to a temp file first so a crash mid-write can't wipe your keys.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("Couldn't save settings: {e}"))?;
    restrict_to_owner(&tmp);
    std::fs::rename(&tmp, path).map_err(|e| format!("Couldn't save settings: {e}"))?;
    Ok(())
  }

  /// The folder images are saved in, falling back to Pictures/Easel.
  pub fn output_dir(&self, pictures_dir: Option<PathBuf>) -> PathBuf {
    let custom = self.output_dir.trim();
    if !custom.is_empty() {
      return PathBuf::from(custom);
    }
    pictures_dir
      .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
      .unwrap_or_else(|| PathBuf::from("."))
      .join("Easel")
  }
}

#[cfg(unix)]
fn restrict_to_owner(path: &Path) {
  use std::os::unix::fs::PermissionsExt;
  let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) {
  // On Windows the config folder is already inside your user profile.
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn missing_fields_fall_back_to_defaults() {
    let s: Settings = serde_json::from_str(r#"{"gemini":{"apiKey":"k"}}"#).unwrap();
    assert_eq!(s.gemini.api_key, "k");
    assert_eq!(s.gemini.image_model, DEFAULT_GEMINI_IMAGE_MODEL);
    assert_eq!(s.comfy.url, DEFAULT_COMFY_URL);
    assert_eq!(s.brain.order, vec![BrainKind::ClaudeCli, BrainKind::Local]);
  }

  #[test]
  fn brains_round_trip_as_plain_names_and_unknown_ones_are_skipped() {
    let s: Settings = serde_json::from_str(r#"{"brain":{"order":["remote:lan","futureBrain","local"],"advisorOrder":["claudeApi"]}}"#).unwrap();
    assert_eq!(s.brain.order, vec![BrainKind::Remote("lan".into()), BrainKind::Local]);
    assert_eq!(s.brain.advisor_order, vec![BrainKind::ClaudeApi]);
    let json = serde_json::to_value(&s.brain).unwrap();
    assert_eq!(json["order"], serde_json::json!(["remote:lan", "local"]));
    assert_eq!(json["advisorMode"], "onRequest");
  }

  #[test]
  fn output_dir_prefers_the_custom_folder() {
    let mut s = Settings::default();
    assert_eq!(s.output_dir(Some(PathBuf::from("/pics"))), PathBuf::from("/pics/Easel"));
    s.output_dir = "/data/art".into();
    assert_eq!(s.output_dir(Some(PathBuf::from("/pics"))), PathBuf::from("/data/art"));
  }

  #[test]
  fn save_then_load_round_trips() {
    let dir = std::env::temp_dir().join(format!("easel-settings-{}", rand::random::<u32>()));
    let path = dir.join("settings.json");
    let mut s = Settings::default();
    s.brain.order = vec![BrainKind::AgyCli];
    s.save(&path).unwrap();
    assert_eq!(Settings::load(&path).brain.order, vec![BrainKind::AgyCli]);
    let _ = std::fs::remove_dir_all(dir);
  }
}
