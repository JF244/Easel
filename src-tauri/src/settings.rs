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
pub const DEFAULT_LOCAL_LLM_URL: &str = "http://127.0.0.1:1234";
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrainKind {
  /// Claude Code in print mode (`claude -p`): uses your Claude subscription.
  ClaudeCli,
  /// Google Antigravity CLI in print mode (`agy -p`): uses your Google subscription.
  AgyCli,
  /// The Anthropic API with your own key: pay per use.
  ClaudeApi,
  /// LM Studio, Ollama or any OpenAI-compatible server on your PC.
  Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BrainSettings {
  pub order: Vec<BrainKind>,
  pub claude_cli_path: String,
  /// Optional model for Claude Code, e.g. "opus" or "sonnet". Empty uses its default.
  pub claude_cli_model: String,
  pub agy_cli_path: String,
  pub claude_api_key: String,
  pub claude_api_model: String,
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
      claude_cli_path: DEFAULT_CLAUDE_CLI.into(),
      claude_cli_model: String::new(),
      agy_cli_path: DEFAULT_AGY_CLI.into(),
      claude_api_key: String::new(),
      claude_api_model: DEFAULT_CLAUDE_MODEL.into(),
      local_url: DEFAULT_LOCAL_LLM_URL.into(),
      local_model: String::new(),
    }
  }
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
